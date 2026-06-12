use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use rand::{seq::SliceRandom, Rng};
use serde::Deserialize;
use calendar_slam_shared::{
    simulate_knockout_match, Attribute, AttributePickDto, Bracket, BracketMatch, EditionDto,
    LeaderboardRow, Level, LobbyPlayer, MpClientMsg, MpServerMsg, MpTeam, PlayerDto, PlayerRatings,
    RunDto, SavedRunDto, SpinDto, Surface,
};
use sqlx::{FromRow, PgPool};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{mpsc, Mutex};

const MP_MAX_BRACKET_SIZE: u8 = 16;
const MP_TURN_MS: u32 = 30_000;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    sessions: Arc<Mutex<HashMap<String, Session>>>,
}

impl AppState {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(|| async { "ok" }))
        .route("/api/spin", get(spin))
        .route("/api/mp/ws", get(mp_ws))
        .route("/api/reroll", get(reroll))
        .route("/api/slam-edition", get(slam_edition))
        .route("/api/runs", post(save_run))
        .route("/api/runs/{id}", get(run_detail))
        .route("/api/leaderboard", get(leaderboard))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
pub struct SpinQuery {
    level: Option<Level>,
    tournament: Option<String>,
    year: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct RerollQuery {
    kind: String,
    level: Level,
    tournament: Option<String>,
    year: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct SlamQuery {
    slam: String,
}

#[derive(Debug, Deserialize)]
pub struct LeaderboardQuery {
    limit: Option<i64>,
}

#[derive(Debug, FromRow)]
struct EditionRow {
    id: i64,
    level: String,
    tournament: String,
    year: i32,
    surface: String,
    champion: String,
    champion_strength: f64,
}

#[derive(Debug, FromRow)]
struct PlayerRow {
    player_name: String,
    best_round: String,
    serve: Option<i16>,
    forehand: Option<i16>,
    backhand: Option<i16>,
    return_rating: Option<i16>,
    movement: Option<i16>,
    mental: Option<i16>,
    net: Option<i16>,
    stamina: Option<i16>,
    trademark: Option<String>,
    trademark_floor: Option<i16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Lobby,
    Draft,
    Knockout,
    Done,
}

#[derive(Debug, Clone)]
struct ServerTeam {
    id: String,
    name: String,
    is_bot: bool,
    connected: bool,
    picks: Vec<AttributePickDto>,
}

#[derive(Debug, Clone)]
struct TurnState {
    team_id: String,
    spin: SpinDto,
    token: u64,
}

#[derive(Debug)]
struct Session {
    code: String,
    host_id: String,
    bracket_size: u8,
    phase: Phase,
    teams: Vec<ServerTeam>,
    senders: HashMap<String, mpsc::UnboundedSender<MpServerMsg>>,
    draft_order: Vec<String>,
    draft_cursor: usize,
    used_players: HashSet<String>,
    current_turn: Option<TurnState>,
    turn_token: u64,
    // Host-driven knockout reveal: how many matches revealed, and the total to reveal.
    reveal: usize,
    knockout_total: usize,
}

async fn spin(
    State(state): State<AppState>,
    Query(query): Query<SpinQuery>,
) -> Result<Json<SpinDto>, StatusCode> {
    Ok(Json(
        draw_spin(&state.pool, query.level, query.tournament, query.year).await?,
    ))
}

async fn reroll(
    State(state): State<AppState>,
    Query(query): Query<RerollQuery>,
) -> Result<Json<SpinDto>, StatusCode> {
    let level_text = level_to_db(query.level);
    let edition = match query.kind.as_str() {
        // Change level: new level + new tournament, SAME year (client picked the new level).
        "level" => {
            let year = query.year.ok_or(StatusCode::BAD_REQUEST)?;
            sqlx::query_as::<_, EditionRow>(
                "SELECT * FROM editions WHERE level = $1 AND year = $2 ORDER BY random() LIMIT 1",
            )
            .bind(level_text)
            .bind(year)
            .fetch_optional(&state.pool)
            .await
        }
        // Change tournament: same level + same year, a different tournament.
        "tournament" => {
            let year = query.year.ok_or(StatusCode::BAD_REQUEST)?;
            let current = query.tournament.ok_or(StatusCode::BAD_REQUEST)?;
            sqlx::query_as::<_, EditionRow>(
                "SELECT * FROM editions
                 WHERE level = $1 AND year = $2 AND tournament <> $3
                 ORDER BY random() LIMIT 1",
            )
            .bind(level_text)
            .bind(year)
            .bind(current)
            .fetch_optional(&state.pool)
            .await
        }
        // Change year: same tournament, a different year.
        "year" => {
            let current = query.year.ok_or(StatusCode::BAD_REQUEST)?;
            let tournament = query.tournament.ok_or(StatusCode::BAD_REQUEST)?;
            sqlx::query_as::<_, EditionRow>(
                "SELECT * FROM editions
                 WHERE level = $1 AND tournament = $2 AND year <> $3
                 ORDER BY random() LIMIT 1",
            )
            .bind(level_text)
            .bind(tournament)
            .bind(current)
            .fetch_optional(&state.pool)
            .await
        }
        _ => return Err(StatusCode::BAD_REQUEST),
    }
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    // No row = no alternative for this reroll (e.g. a tournament with a single year).
    .ok_or(StatusCode::NOT_FOUND)?;

    let edition_level = parse_level(&edition.level);
    let players = players_for_edition(&state.pool, edition.id, edition_level, None).await?;

    Ok(Json(SpinDto {
        level: edition_level,
        tournament: edition.tournament,
        year: edition.year,
        surface: parse_surface(&edition.surface),
        players,
    }))
}

async fn slam_edition(
    State(state): State<AppState>,
    Query(query): Query<SlamQuery>,
) -> Result<Json<EditionDto>, StatusCode> {
    let tournament = match query.slam.as_str() {
        "AO" => "Australian Open",
        "RG" => "Roland Garros",
        "WIM" => "Wimbledon",
        "USO" => "US Open",
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    let row = sqlx::query_as::<_, EditionRow>(
        "SELECT * FROM editions
         WHERE level = 'GrandSlam' AND tournament = $1
         ORDER BY random()
         LIMIT 1",
    )
    .bind(tournament)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::NOT_FOUND)?;

    // Opponents get a small flat boost over their natural edition strength (tunable difficulty knob).
    const OPPONENT_BOOST: i16 = 2;
    let players: Vec<PlayerDto> = players_for_edition(&state.pool, row.id, Level::ATP500, Some("W"))
        .await?
        .into_iter()
        .map(|mut player| {
            player.ratings = player.ratings.plus_all(OPPONENT_BOOST);
            player
        })
        .collect();

    Ok(Json(EditionDto {
        slam: query.slam,
        tournament: row.tournament,
        year: row.year,
        surface: parse_surface(&row.surface),
        champion: row.champion,
        champion_strength: row.champion_strength,
        players,
    }))
}

async fn save_run(
    State(state): State<AppState>,
    Json(mut run): Json<RunDto>,
) -> Result<Json<SavedRunDto>, StatusCode> {
    run.nickname = clean_nickname(&run.nickname);
    if run.nickname.is_empty() || run.nickname.chars().count() > 32 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let attributes = serde_json::to_value(&run.attributes).map_err(|_| StatusCode::BAD_REQUEST)?;
    let sources = serde_json::to_value(&run.sources).map_err(|_| StatusCode::BAD_REQUEST)?;

    // Trust the data, not the claimed total: recompute points from the saved slam sources.
    let points = calendar_slam_shared::run_points(&run.sources) as i32;

    let saved = sqlx::query_as::<_, SavedRunRow>(
        "INSERT INTO runs (nickname, overall, slams_won, points, attributes, sources)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, created_at",
    )
    .bind(run.nickname)
    .bind(i32::from(run.overall))
    .bind(i32::from(run.slams_won))
    .bind(points)
    .bind(attributes)
    .bind(sources)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(SavedRunDto {
        id: saved.id,
        created_at: saved.created_at,
    }))
}

async fn run_detail(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<RunDto>, StatusCode> {
    let row = sqlx::query_as::<_, RunDetailRow>(
        "SELECT nickname, overall, slams_won, points, attributes, sources
         FROM runs WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    let attributes = serde_json::from_value(row.attributes).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let sources = serde_json::from_value(row.sources).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(RunDto {
        nickname: row.nickname,
        overall: row.overall.max(0) as u8,
        slams_won: row.slams_won.max(0) as u8,
        points: row.points.max(0) as u32,
        attributes,
        sources,
    }))
}

async fn leaderboard(
    State(state): State<AppState>,
    Query(query): Query<LeaderboardQuery>,
) -> Result<Json<Vec<LeaderboardRow>>, StatusCode> {
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let rows = sqlx::query_as::<_, LeaderboardDbRow>(
        "SELECT id, nickname, overall, slams_won, points, created_at
         FROM runs
         ORDER BY points DESC, overall DESC, created_at ASC
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(
        rows.into_iter()
            .map(|row| LeaderboardRow {
                id: row.id,
                nickname: row.nickname,
                overall: row.overall as u8,
                slams_won: row.slams_won as u8,
                points: row.points.max(0) as u32,
                created_at: row.created_at,
            })
            .collect(),
    ))
}

async fn mp_ws(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_mp_socket(socket, state))
}

async fn handle_mp_socket(socket: WebSocket, state: AppState) {
    let connection_id = random_id("p");
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<MpServerMsg>();
    let mut room_code: Option<String> = None;
    let mut team_id: Option<String> = None;

    let writer = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            let Ok(text) = serde_json::to_string(&message) else {
                continue;
            };
            if ws_tx.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(message)) = ws_rx.next().await {
        let Message::Text(text) = message else {
            continue;
        };
        let parsed = serde_json::from_str::<MpClientMsg>(&text);
        let Ok(client_msg) = parsed else {
            let _ = tx.send(MpServerMsg::Error {
                message: "Mensagem invalida.".to_string(),
            });
            continue;
        };

        match client_msg {
            MpClientMsg::CreateRoom { name, bracket_size } => {
                let clean = clean_player_name(&name);
                let bracket_size = bracket_size.clamp(8, MP_MAX_BRACKET_SIZE);
                if ![8, 16].contains(&bracket_size) {
                    let _ = tx.send(MpServerMsg::Error {
                        message: "Tamanho maximo atual: 16 jogadores.".to_string(),
                    });
                    continue;
                }
                let code = unique_room_code(&state).await;
                let id = connection_id.clone();
                let team = ServerTeam {
                    id: id.clone(),
                    name: clean,
                    is_bot: false,
                    connected: true,
                    picks: Vec::new(),
                };
                let session = Session {
                    code: code.clone(),
                    host_id: id.clone(),
                    bracket_size,
                    phase: Phase::Lobby,
                    teams: vec![team],
                    senders: HashMap::from([(id.clone(), tx.clone())]),
                    draft_order: Vec::new(),
                    draft_cursor: 0,
                    used_players: HashSet::new(),
                    current_turn: None,
                    turn_token: 0,
                    reveal: 0,
                    knockout_total: 0,
                };
                let joined = MpServerMsg::Joined {
                    your_id: id.clone(),
                    code: code.clone(),
                };
                let _ = tx.send(joined);
                broadcast_room_state(&session);
                state.sessions.lock().await.insert(code.clone(), session);
                room_code = Some(code);
                team_id = Some(id);
            }
            MpClientMsg::JoinRoom { code, name } => {
                let code = code.trim().to_ascii_uppercase();
                let mut sessions = state.sessions.lock().await;
                let Some(session) = sessions.get_mut(&code) else {
                    let _ = tx.send(MpServerMsg::Error {
                        message: "Sala nao encontrada.".to_string(),
                    });
                    continue;
                };
                if session.phase != Phase::Lobby {
                    let _ = tx.send(MpServerMsg::Error {
                        message: "Essa sala ja comecou.".to_string(),
                    });
                    continue;
                }
                if session.teams.len() >= session.bracket_size as usize {
                    let _ = tx.send(MpServerMsg::Error {
                        message: "Sala cheia.".to_string(),
                    });
                    continue;
                }
                let id = connection_id.clone();
                session.teams.push(ServerTeam {
                    id: id.clone(),
                    name: clean_player_name(&name),
                    is_bot: false,
                    connected: true,
                    picks: Vec::new(),
                });
                session.senders.insert(id.clone(), tx.clone());
                let _ = tx.send(MpServerMsg::Joined {
                    your_id: id.clone(),
                    code: code.clone(),
                });
                broadcast_room_state(session);
                room_code = Some(code);
                team_id = Some(id);
            }
            MpClientMsg::StartGame => {
                let Some(code) = room_code.clone() else {
                    continue;
                };
                let mut sessions = state.sessions.lock().await;
                let Some(session) = sessions.get_mut(&code) else {
                    continue;
                };
                if team_id.as_deref() != Some(session.host_id.as_str()) {
                    let _ = tx.send(MpServerMsg::Error {
                        message: "Apenas o host inicia a sala.".to_string(),
                    });
                    continue;
                }
                if session.phase != Phase::Lobby {
                    continue;
                }
                fill_bots(session);
                session.phase = Phase::Draft;
                session.draft_order = snake_order(&session.teams);
                session.draft_cursor = 0;
                session.used_players.clear();
                session.current_turn = None;
                broadcast_room_state(session);
                drop(sessions);
                spawn_advance(state.clone(), code);
            }
            MpClientMsg::MakePick { attribute, player } => {
                let (Some(code), Some(id)) = (room_code.clone(), team_id.clone()) else {
                    continue;
                };
                let accepted = record_human_pick(&state, &code, &id, attribute, &player).await;
                if accepted {
                    spawn_advance(state.clone(), code);
                }
            }
            // Host advances the knockout reveal; the new count is broadcast to everyone.
            MpClientMsg::RevealNext => {
                let (Some(code), Some(id)) = (room_code.clone(), team_id.clone()) else {
                    continue;
                };
                let mut sessions = state.sessions.lock().await;
                let Some(session) = sessions.get_mut(&code) else {
                    continue;
                };
                if session.phase != Phase::Knockout {
                    continue;
                }
                if session.host_id != id {
                    let _ = tx.send(MpServerMsg::Error {
                        message: "Apenas o host avanca os jogos.".to_string(),
                    });
                    continue;
                }
                if session.reveal < session.knockout_total {
                    session.reveal += 1;
                    if session.reveal >= session.knockout_total {
                        session.phase = Phase::Done;
                    }
                    let reveal = session.reveal as u32;
                    broadcast(session, MpServerMsg::RevealAdvance { reveal });
                }
            }
        }
    }

    if let (Some(code), Some(id)) = (room_code, team_id) {
        disconnect_from_room(&state, &code, &id).await;
    }
    writer.abort();
}

fn spawn_advance(state: AppState, code: String) {
    tokio::spawn(async move {
        advance_draft(state, code).await;
    });
}

async fn advance_draft(state: AppState, code: String) {
    loop {
        let (used, team_id, is_bot) = {
            let mut sessions = state.sessions.lock().await;
            let Some(session) = sessions.get_mut(&code) else {
                return;
            };
            if session.phase != Phase::Draft || session.current_turn.is_some() {
                return;
            }
            if session.draft_cursor >= session.draft_order.len() {
                let (bracket, champion) = simulate_bracket(&session.teams);
                let teams = session.teams.iter().map(server_team_to_dto).collect();
                // Enter the knockout: the host reveals matches one at a time (synced to all).
                session.knockout_total = bracket.rounds.iter().map(|round| round.len()).sum();
                session.reveal = 0;
                session.phase = Phase::Knockout;
                broadcast(
                    session,
                    MpServerMsg::KnockoutResult {
                        bracket,
                        champion,
                        teams,
                    },
                );
                return;
            }
            let team_id = session.draft_order[session.draft_cursor].clone();
            let is_bot = session
                .teams
                .iter()
                .find(|team| team.id == team_id)
                .map(|team| team.is_bot)
                .unwrap_or(false);
            (session.used_players.clone(), team_id, is_bot)
        };

        let spin = match draw_available_spin(&state.pool, &used).await {
            Ok(spin) => spin,
            Err(_) => {
                let mut sessions = state.sessions.lock().await;
                if let Some(session) = sessions.get_mut(&code) {
                    broadcast(
                        session,
                        MpServerMsg::Error {
                            message: "Nao foi possivel sortear jogadores disponiveis.".to_string(),
                        },
                    );
                }
                return;
            }
        };
        // Keep the tournament-performance order from `players_for_edition` (furthest round first);
        // do NOT re-sort alphabetically.

        let token = {
            let mut sessions = state.sessions.lock().await;
            let Some(session) = sessions.get_mut(&code) else {
                return;
            };
            if session.current_turn.is_some() || session.phase != Phase::Draft {
                return;
            }
            session.turn_token = session.turn_token.saturating_add(1);
            let token = session.turn_token;
            session.current_turn = Some(TurnState {
                team_id: team_id.clone(),
                spin: spin.clone(),
                token,
            });
            broadcast_draft_turn(session, &team_id, &spin);
            token
        };

        if is_bot {
            tokio::time::sleep(Duration::from_millis(450)).await;
            if auto_pick(&state, &code, token).await {
                continue;
            }
        } else {
            let state_for_timer = state.clone();
            let code_for_timer = code.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(u64::from(MP_TURN_MS))).await;
                if auto_pick(&state_for_timer, &code_for_timer, token).await {
                    spawn_advance(state_for_timer, code_for_timer);
                }
            });
        }
        return;
    }
}

async fn draw_available_spin(pool: &PgPool, used: &HashSet<String>) -> Result<SpinDto, StatusCode> {
    for _ in 0..12 {
        let mut spin = draw_spin(pool, None, None, None).await?;
        spin.players
            .retain(|player| !used.contains(&player.name.to_lowercase()));
        if !spin.players.is_empty() {
            return Ok(spin);
        }
    }
    Err(StatusCode::NOT_FOUND)
}

async fn record_human_pick(
    state: &AppState,
    code: &str,
    team_id: &str,
    attribute: Attribute,
    player: &str,
) -> bool {
    let mut sessions = state.sessions.lock().await;
    let Some(session) = sessions.get_mut(code) else {
        return false;
    };
    let Some(turn) = session.current_turn.clone() else {
        return false;
    };
    if turn.team_id != team_id {
        return false;
    }
    let player_dto = turn
        .spin
        .players
        .iter()
        .find(|candidate| candidate.name == player)
        .cloned();
    let Some(player_dto) = player_dto else {
        return false;
    };
    let Some(team) = session.teams.iter_mut().find(|team| team.id == team_id) else {
        return false;
    };
    if team.is_bot || team.picks.iter().any(|pick| pick.attribute == attribute) {
        return false;
    }
    let rating = player_dto.ratings.get(attribute);
    team.picks.push(AttributePickDto {
        attribute,
        player: player_dto.name.clone(),
        rating,
        source: turn.spin,
    });
    session.used_players.insert(player_dto.name.to_lowercase());
    session.current_turn = None;
    session.draft_cursor += 1;
    broadcast(
        session,
        MpServerMsg::PickMade {
            team_id: team_id.to_string(),
            attribute,
            player: player_dto.name,
            rating,
        },
    );
    true
}

async fn auto_pick(state: &AppState, code: &str, token: u64) -> bool {
    let mut sessions = state.sessions.lock().await;
    let Some(session) = sessions.get_mut(code) else {
        return false;
    };
    let Some(turn) = session.current_turn.clone() else {
        return false;
    };
    if turn.token != token {
        return false;
    }
    let Some(team_index) = session.teams.iter().position(|team| team.id == turn.team_id) else {
        return false;
    };
    let Some(attribute) = Attribute::ALL.into_iter().find(|attribute| {
        !session.teams[team_index]
            .picks
            .iter()
            .any(|pick| pick.attribute == *attribute)
    }) else {
        return false;
    };
    let Some(player) = turn
        .spin
        .players
        .iter()
        .max_by_key(|player| player.ratings.get(attribute))
        .cloned()
    else {
        return false;
    };
    let rating = player.ratings.get(attribute);
    session.teams[team_index].picks.push(AttributePickDto {
        attribute,
        player: player.name.clone(),
        rating,
        source: turn.spin,
    });
    session.used_players.insert(player.name.to_lowercase());
    session.current_turn = None;
    session.draft_cursor += 1;
    broadcast(
        session,
        MpServerMsg::PickMade {
            team_id: turn.team_id,
            attribute,
            player: player.name,
            rating,
        },
    );
    true
}

async fn disconnect_from_room(state: &AppState, code: &str, team_id: &str) {
    let mut should_remove = false;
    let mut autopick_token: Option<u64> = None;
    {
        let mut sessions = state.sessions.lock().await;
        if let Some(session) = sessions.get_mut(code) {
            session.senders.remove(team_id);
            // The leaver keeps playing as a CPU that auto-picks from here on.
            if let Some(team) = session.teams.iter_mut().find(|team| team.id == team_id) {
                team.is_bot = true;
                team.connected = true;
            }
            // If the host left, hand the host role to a still-connected human.
            if session.host_id == team_id {
                let humans: Vec<String> = session
                    .teams
                    .iter()
                    .filter(|team| !team.is_bot)
                    .map(|team| team.id.clone())
                    .collect();
                if let Some(new_host) = humans
                    .into_iter()
                    .find(|id| session.senders.contains_key(id))
                {
                    session.host_id = new_host;
                }
            }
            if session.senders.is_empty() {
                should_remove = true;
            } else {
                // If it was the leaver's turn to draft, auto-pick immediately (don't wait 30s).
                if session.phase == Phase::Draft {
                    if let Some(turn) = &session.current_turn {
                        if turn.team_id == team_id {
                            autopick_token = Some(turn.token);
                        }
                    }
                }
                broadcast_room_state(session);
            }
        }
        if should_remove {
            sessions.remove(code);
        }
    }
    if let Some(token) = autopick_token {
        if auto_pick(state, code, token).await {
            spawn_advance(state.clone(), code.to_string());
        }
    }
}

fn server_team_to_dto(team: &ServerTeam) -> MpTeam {
    MpTeam {
        id: team.id.clone(),
        name: team.name.clone(),
        is_bot: team.is_bot,
        picks: team.picks.clone(),
    }
}

fn fill_bots(session: &mut Session) {
    let mut bot_num = 1;
    while session.teams.len() < session.bracket_size as usize {
        session.teams.push(ServerTeam {
            id: format!("bot-{bot_num}"),
            name: format!("CPU {bot_num}"),
            is_bot: true,
            connected: true,
            picks: Vec::new(),
        });
        bot_num += 1;
    }
}

fn snake_order(teams: &[ServerTeam]) -> Vec<String> {
    let mut order = Vec::with_capacity(teams.len() * Attribute::ALL.len());
    let ids = teams.iter().map(|team| team.id.clone()).collect::<Vec<_>>();
    for round in 0..Attribute::ALL.len() {
        if round % 2 == 0 {
            order.extend(ids.iter().cloned());
        } else {
            order.extend(ids.iter().rev().cloned());
        }
    }
    order
}

fn broadcast_room_state(session: &Session) {
    broadcast(
        session,
        MpServerMsg::RoomState {
            code: session.code.clone(),
            host_id: session.host_id.clone(),
            bracket_size: session.bracket_size,
            players: session
                .teams
                .iter()
                .map(|team| LobbyPlayer {
                    id: team.id.clone(),
                    name: team.name.clone(),
                    is_bot: team.is_bot,
                    connected: team.connected,
                })
                .collect(),
        },
    );
}

fn broadcast_draft_turn(session: &Session, on_clock: &str, spin: &SpinDto) {
    let picks_made = session.draft_cursor.min(u8::MAX as usize) as u8;
    let total_picks = session.draft_order.len().min(u8::MAX as usize) as u8;
    for (id, sender) in &session.senders {
        let _ = sender.send(MpServerMsg::DraftTurn {
            on_clock: on_clock.to_string(),
            your_turn: id == on_clock,
            spin: spin.clone(),
            deadline_ms: MP_TURN_MS,
            picks_made,
            total_picks,
        });
    }
}

fn broadcast(session: &Session, message: MpServerMsg) {
    for sender in session.senders.values() {
        let _ = sender.send(message.clone());
    }
}

fn simulate_bracket(teams: &[ServerTeam]) -> (Bracket, String) {
    let mut rng = rand::thread_rng();
    let mut current = teams.to_vec();
    current.shuffle(&mut rng);
    let mut rounds = Vec::new();
    let mut round_index = 1u8;

    while current.len() > 1 {
        // Every knockout match is best-of-5.
        let best_of = 5;
        let mut matches = Vec::new();
        let mut winners = Vec::new();
        for pair in current.chunks(2) {
            if pair.len() < 2 {
                winners.push(pair[0].clone());
                continue;
            }
            let surface = random_slam_surface(&mut rng);
            let outcome =
                simulate_knockout_match(&pair[0].picks, &pair[1].picks, surface, best_of, &mut rng);
            let winner = if outcome.a_wins { pair[0].clone() } else { pair[1].clone() };
            matches.push(BracketMatch {
                round: round_index,
                a: pair[0].id.clone(),
                b: pair[1].id.clone(),
                winner: Some(winner.id.clone()),
                surface,
                sets: outcome.sets,
                games: outcome.games,
            });
            winners.push(winner);
        }
        rounds.push(matches);
        current = winners;
        round_index += 1;
    }

    let champion = current
        .first()
        .map(|team| team.id.clone())
        .unwrap_or_else(|| "none".to_string());
    (Bracket { rounds }, champion)
}

fn random_slam_surface(rng: &mut impl Rng) -> Surface {
    match rng.gen_range(0..4) {
        0 => Surface::Clay,
        1 => Surface::Grass,
        _ => Surface::Hard,
    }
}

async fn unique_room_code(state: &AppState) -> String {
    loop {
        let code = random_code();
        if !state.sessions.lock().await.contains_key(&code) {
            return code;
        }
    }
}

fn random_code() -> String {
    const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut rng = rand::thread_rng();
    (0..5)
        .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
        .collect()
}

fn random_id(prefix: &str) -> String {
    let mut rng = rand::thread_rng();
    format!("{prefix}-{:08x}", rng.gen::<u32>())
}

fn clean_player_name(value: &str) -> String {
    let name = value.trim().chars().take(24).collect::<String>();
    if name.is_empty() {
        "Jogador".to_string()
    } else {
        name
    }
}

#[derive(Debug, FromRow)]
struct SavedRunRow {
    id: i64,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, FromRow)]
struct RunDetailRow {
    nickname: String,
    overall: i32,
    slams_won: i32,
    points: i32,
    attributes: serde_json::Value,
    sources: serde_json::Value,
}

#[derive(Debug, FromRow)]
struct LeaderboardDbRow {
    id: i64,
    nickname: String,
    overall: i32,
    slams_won: i32,
    points: i32,
    created_at: chrono::DateTime<chrono::Utc>,
}

async fn draw_spin(
    pool: &PgPool,
    level: Option<Level>,
    tournament: Option<String>,
    year: Option<i32>,
) -> Result<SpinDto, StatusCode> {
    let level = level.unwrap_or_else(weighted_level);
    if level == Level::ATP250 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let level_text = level_to_db(level);

    let edition = if let (Some(tournament), Some(year)) = (tournament.as_deref(), year) {
        sqlx::query_as::<_, EditionRow>(
            "SELECT * FROM editions
             WHERE level = $1 AND tournament = $2 AND year = $3
             LIMIT 1",
        )
        .bind(level_text)
        .bind(tournament)
        .bind(year)
        .fetch_one(pool)
        .await
    } else if let Some(tournament) = tournament {
        sqlx::query_as::<_, EditionRow>(
            "SELECT * FROM editions
             WHERE level = $1 AND tournament = $2
             ORDER BY random()
             LIMIT 1",
        )
        .bind(level_text)
        .bind(tournament)
        .fetch_one(pool)
        .await
    } else {
        sqlx::query_as::<_, EditionRow>(
            "SELECT * FROM editions
             WHERE level = $1
             ORDER BY random()
             LIMIT 1",
        )
        .bind(level_text)
        .fetch_one(pool)
        .await
    }
    .map_err(|_| StatusCode::NOT_FOUND)?;

    let edition_level = parse_level(&edition.level);
    let players = players_for_edition(pool, edition.id, edition_level, None).await?;

    Ok(SpinDto {
        level: edition_level,
        tournament: edition.tournament,
        year: edition.year,
        surface: parse_surface(&edition.surface),
        players,
    })
}

async fn players_for_edition(
    pool: &PgPool,
    edition_id: i64,
    level: Level,
    // When set, every player is buffed by this round (used to give slam opponents the champion boost)
    // instead of their own `best_round`.
    force_round: Option<&str>,
) -> Result<Vec<PlayerDto>, StatusCode> {
    let rows = sqlx::query_as::<_, PlayerRow>(
        "SELECT
             ep.player_name,
             ep.best_round,
             pp.serve,
             pp.forehand,
             pp.backhand,
             pp.return_rating,
             pp.movement,
             pp.mental,
             pp.net,
             pp.stamina,
             pp.trademark,
             pp.trademark_floor
         FROM edition_players ep
         LEFT JOIN player_profiles pp ON pp.player_name = ep.player_name
         WHERE ep.edition_id = $1
         ORDER BY CASE best_round
             WHEN 'W' THEN 1
             WHEN 'F' THEN 2
             WHEN 'SF' THEN 3
             WHEN 'QF' THEN 4
             WHEN 'R16' THEN 5
             ELSE 6
         END, player_name",
    )
    .bind(edition_id)
    .fetch_all(pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let mut ratings = PlayerRatings {
                serve: opt_rating(row.serve, 55),
                forehand: opt_rating(row.forehand, 55),
                backhand: opt_rating(row.backhand, 55),
                return_rating: opt_rating(row.return_rating, 55),
                movement: opt_rating(row.movement, 55),
                mental: opt_rating(row.mental, 55),
                net: opt_rating(row.net, 55),
                stamina: opt_rating(row.stamina, 55),
            }
            .buffed(level, force_round.unwrap_or(&row.best_round));

            if let (Some(trademark), Some(floor)) = (&row.trademark, row.trademark_floor) {
                if let Some(attribute) = calendar_slam_shared::Attribute::from_key(trademark) {
                    ratings = ratings.floored(attribute, opt_rating(Some(floor), 0));
                }
            }

            PlayerDto {
                name: row.player_name,
                best_round: row.best_round.clone(),
                ratings,
            }
        })
        .collect())
}

fn weighted_level() -> Level {
    let roll = rand::thread_rng().gen_range(0..100);
    match roll {
        0..=49 => Level::ATP500,
        50..=79 => Level::ATP1000,
        _ => Level::GrandSlam,
    }
}

fn opt_rating(value: Option<i16>, fallback: u8) -> u8 {
    value
        .map(|value| value.clamp(0, 99) as u8)
        .unwrap_or(fallback)
}

fn level_to_db(level: Level) -> &'static str {
    match level {
        Level::ATP250 => "ATP250",
        Level::ATP500 => "ATP500",
        Level::ATP1000 => "ATP1000",
        Level::GrandSlam => "GrandSlam",
    }
}

fn parse_level(level: &str) -> Level {
    match level {
        "ATP500" => Level::ATP500,
        "ATP1000" => Level::ATP1000,
        "GrandSlam" => Level::GrandSlam,
        _ => Level::ATP250,
    }
}

fn parse_surface(surface: &str) -> Surface {
    match surface {
        "Clay" => Surface::Clay,
        "Grass" => Surface::Grass,
        "Carpet" => Surface::Carpet,
        _ => Surface::Hard,
    }
}

fn clean_nickname(value: &str) -> String {
    value.trim().chars().take(32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weighted_level_never_returns_atp250() {
        for _ in 0..1000 {
            assert_ne!(weighted_level(), Level::ATP250);
        }
    }

    #[test]
    fn opt_rating_clamps_and_falls_back() {
        assert_eq!(opt_rating(Some(120), 55), 99);
        assert_eq!(opt_rating(Some(-4), 55), 0);
        assert_eq!(opt_rating(None, 55), 55);
    }
}
