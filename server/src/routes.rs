use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use rand::Rng;
use serde::Deserialize;
use calendar_slam_shared::{
    EditionDto, LeaderboardRow, Level, PlayerDto, PlayerRatings, RunDto, SavedRunDto, SpinDto,
    Surface,
};
use sqlx::{FromRow, PgPool};

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
}

pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(|| async { "ok" }))
        .route("/api/spin", get(spin))
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

async fn spin(
    State(state): State<AppState>,
    Query(query): Query<SpinQuery>,
) -> Result<Json<SpinDto>, StatusCode> {
    let level = query.level.unwrap_or_else(weighted_level);
    if level == Level::ATP250 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let level_text = level_to_db(level);

    let edition = if let (Some(tournament), Some(year)) = (query.tournament.as_deref(), query.year) {
        sqlx::query_as::<_, EditionRow>(
            "SELECT * FROM editions
             WHERE level = $1 AND tournament = $2 AND year = $3
             LIMIT 1",
        )
        .bind(level_text)
        .bind(tournament)
        .bind(year)
        .fetch_one(&state.pool)
        .await
    } else if let Some(tournament) = query.tournament {
        sqlx::query_as::<_, EditionRow>(
            "SELECT * FROM editions
             WHERE level = $1 AND tournament = $2
             ORDER BY random()
             LIMIT 1",
        )
        .bind(level_text)
        .bind(tournament)
        .fetch_one(&state.pool)
        .await
    } else {
        sqlx::query_as::<_, EditionRow>(
            "SELECT * FROM editions
             WHERE level = $1
             ORDER BY random()
             LIMIT 1",
        )
        .bind(level_text)
        .fetch_one(&state.pool)
        .await
    }
    .map_err(|_| StatusCode::NOT_FOUND)?;

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
