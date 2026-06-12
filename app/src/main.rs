mod api;
mod mp;
mod share;
mod sim;

use calendar_slam_shared::{
    Attribute, AttributePickDto, Bracket, BracketMatch, LeaderboardRow, Level, LobbyPlayer,
    MpClientMsg, MpServerMsg, MpTeam, PlayerDto, RunDto, SlamResultDto, SpinDto, Surface,
};
use rand::Rng;
use wasm_bindgen_futures::spawn_local;
use web_sys::{HtmlCanvasElement, HtmlInputElement, HtmlSelectElement};
use yew::prelude::*;

// Slams played one at a time, in calendar order.
const SLAMS: [(&str, &str); 4] = [
    ("AO", "Australian Open"),
    ("RG", "Roland Garros"),
    ("WIM", "Wimbledon"),
    ("USO", "US Open"),
];

// Reroll budget for the WHOLE run (all 8 picks), not per spin.
const LEVEL_REROLLS: u8 = 1;
const TOURNAMENT_REROLLS: u8 = 2;
const YEAR_REROLLS: u8 = 2;

#[derive(Default)]
struct RerollBudget {
    level_used: u8,
    tournament_used: u8,
    year_used: u8,
}

#[derive(Clone, Copy)]
enum RerollKind {
    Level,
    Tournament,
    Year,
}

impl RerollKind {
    fn as_str(self) -> &'static str {
        match self {
            RerollKind::Level => "level",
            RerollKind::Tournament => "tournament",
            RerollKind::Year => "year",
        }
    }
}

#[derive(Default, PartialEq, Clone, Copy)]
enum Language {
    #[default]
    Pt,
    En,
}

impl Language {
    fn code(self) -> &'static str {
        match self {
            Language::Pt => "PT",
            Language::En => "EN",
        }
    }

    fn other(self) -> Self {
        match self {
            Language::Pt => Language::En,
            Language::En => Language::Pt,
        }
    }

    fn attr(self, attribute: Attribute) -> &'static str {
        match self {
            Language::Pt => attribute.label(),
            Language::En => match attribute {
                Attribute::Serve => "Serve",
                Attribute::Forehand => "Forehand",
                Attribute::Backhand => "Backhand",
                Attribute::Return => "Return",
                Attribute::Movement => "Movement",
                Attribute::Mental => "Mental",
                Attribute::Net => "Net play",
                Attribute::Stamina => "Stamina",
            },
        }
    }

    fn surface(self, surface: Surface) -> &'static str {
        match self {
            Language::Pt => match surface {
                Surface::Hard => "Dura",
                Surface::Clay => "Saibro",
                Surface::Grass => "Grama",
                Surface::Carpet => "Carpete",
            },
            Language::En => match surface {
                Surface::Hard => "Hard",
                Surface::Clay => "Clay",
                Surface::Grass => "Grass",
                Surface::Carpet => "Carpet",
            },
        }
    }
}

// Which screen is on: the landing page (tutorial + ranking) or the draft/sim game.
#[derive(Default, PartialEq, Clone, Copy)]
enum View {
    #[default]
    Home,
    Game,
    MpLobby,
    MpRoom,
    MpDraft,
    MpKnockout,
}

#[derive(Default)]
struct Model {
    language: Language,
    view: View,
    picks: Vec<AttributePickDto>,
    spin: Option<SpinDto>,
    selected_attribute: Option<Attribute>,
    selected_player: Option<PlayerDto>,
    rerolls: RerollBudget,
    loading: bool,
    error: Option<String>,
    sim_results: Vec<SlamResultDto>,
    current_slam: usize,
    revealed_rounds: usize,
    revealing: bool,
    nickname: String,
    leaderboard: Vec<LeaderboardRow>,
    saved: bool,
    // Ranking: which run's build is expanded, and its loaded detail.
    open_run: Option<i64>,
    run_detail: Option<RunDto>,
    share_canvas: NodeRef,
    mp_connection: Option<mp::MpConnection>,
    // Connection generation: messages from a previous (closed) connection are ignored.
    mp_gen: u32,
    mp_name: String,
    mp_join_code: String,
    mp_bracket_size: u8,
    mp_your_id: Option<String>,
    mp_code: Option<String>,
    mp_host_id: Option<String>,
    mp_players: Vec<LobbyPlayer>,
    mp_teams: Vec<MpTeam>,
    mp_spin: Option<SpinDto>,
    mp_on_clock: Option<String>,
    mp_your_turn: bool,
    mp_picks_made: u8,
    mp_total_picks: u8,
    mp_selected_attribute: Option<Attribute>,
    mp_selected_player: Option<PlayerDto>,
    mp_bracket: Option<Bracket>,
    mp_champion: Option<String>,
    // Draft console (who picked whom) and the live autopick countdown.
    mp_log: Vec<String>,
    mp_remaining: u32,
    mp_turn_seq: u32,
    // Knockout: how many matches have been revealed (advanced by button), and which match's
    // attribute head-to-head is open.
    mp_reveal: usize,
    mp_selected_match: Option<usize>,
    // Game-by-game animation of the open match: running (set_index, a_games, b_games) per game.
    mp_game_seq: Vec<(usize, u8, u8)>,
    mp_game_shown: usize,
    mp_anim_seq: u32,
    // Set once the final has finished playing out, so the champion stays shown even if you
    // click back to replay an earlier match.
    mp_finished: bool,
    // Whether the "leave game?" confirmation modal is open.
    mp_confirm_leave: bool,
}

enum Msg {
    ToggleLanguage,
    StartGame,
    GoHome,
    NewSpin,
    SpinLoaded(Result<SpinDto, String>),
    RerollLevel,
    RerollTournament,
    RerollYear,
    RerollResult(RerollKind, Result<SpinDto, String>),
    SelectAttribute(Attribute),
    SelectPlayer(PlayerDto),
    ConfirmPick,
    SimulateSlam,
    SlamSimulated(Result<SlamResultDto, String>),
    RevealRound,
    NicknameChanged(String),
    SaveRun,
    DownloadImage,
    ShareImage,
    RunSaved(Result<(), String>),
    LeaderboardLoaded(Result<Vec<LeaderboardRow>, String>),
    ToggleRun(i64),
    RunDetailLoaded(Result<RunDto, String>),
    OpenMpLobby,
    MpNameChanged(String),
    MpCodeChanged(String),
    MpBracketChanged(u8),
    MpCreateRoom,
    MpJoinRoom,
    MpStartGame,
    MpReceived(u32, Result<MpServerMsg, String>),
    MpSelectAttribute(Attribute),
    MpSelectPlayer(PlayerDto),
    MpConfirmPick,
    MpTick(u32),
    MpRevealNext,
    MpSelectMatch(usize),
    MpGameTick(u32),
    MpRequestLeave,
    MpCancelLeave,
}

impl Component for Model {
    type Message = Msg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        // Start on the home page; load the general ranking so it's there on first paint.
        let link = ctx.link().clone();
        spawn_local(async move {
            link.send_message(Msg::LeaderboardLoaded(api::leaderboard().await));
        });
        Self {
            mp_bracket_size: 8,
            ..Self::default()
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::ToggleLanguage => {
                self.language = self.language.other();
                true
            }
            // Enter the game fresh: wipe any previous run's state and start the first spin.
            Msg::StartGame => {
                self.view = View::Game;
                self.picks.clear();
                self.spin = None;
                self.selected_attribute = None;
                self.selected_player = None;
                self.rerolls = RerollBudget::default();
                self.sim_results.clear();
                self.current_slam = 0;
                self.revealed_rounds = 0;
                self.revealing = false;
                self.nickname.clear();
                self.saved = false;
                self.error = None;
                ctx.link().send_message(Msg::NewSpin);
                true
            }
            Msg::GoHome => {
                self.view = View::Home;
                self.leave_multiplayer();
                let link = ctx.link().clone();
                spawn_local(async move {
                    link.send_message(Msg::LeaderboardLoaded(api::leaderboard().await));
                });
                true
            }
            Msg::NewSpin => {
                self.loading = true;
                fetch_spin(ctx, None, None);
                true
            }
            Msg::SpinLoaded(result) => {
                self.loading = false;
                match result {
                    Ok(spin) => {
                        self.spin = Some(spin);
                        self.selected_player = None;
                        self.selected_attribute = next_attribute(&self.picks);
                        self.error = None;
                    }
                    Err(error) => self.error = Some(error),
                }
                true
            }
            // Change level: keep the YEAR, randomize to a different level + a tournament of it.
            Msg::RerollLevel => {
                if self.rerolls.level_used < LEVEL_REROLLS {
                    if let Some(spin) = &self.spin {
                        self.rerolls.level_used += 1;
                        self.loading = true;
                        let new_level = other_level(spin.level);
                        fetch_reroll(ctx, RerollKind::Level, new_level, None, Some(spin.year));
                    }
                }
                true
            }
            // Change tournament: keep the level AND year, a different tournament.
            Msg::RerollTournament => {
                if self.rerolls.tournament_used < TOURNAMENT_REROLLS {
                    if let Some(spin) = &self.spin {
                        self.rerolls.tournament_used += 1;
                        self.loading = true;
                        fetch_reroll(
                            ctx,
                            RerollKind::Tournament,
                            spin.level,
                            Some(spin.tournament.clone()),
                            Some(spin.year),
                        );
                    }
                }
                true
            }
            // Change year: keep the tournament, a different year.
            Msg::RerollYear => {
                if self.rerolls.year_used < YEAR_REROLLS {
                    if let Some(spin) = &self.spin {
                        self.rerolls.year_used += 1;
                        self.loading = true;
                        fetch_reroll(
                            ctx,
                            RerollKind::Year,
                            spin.level,
                            Some(spin.tournament.clone()),
                            Some(spin.year),
                        );
                    }
                }
                true
            }
            Msg::RerollResult(kind, result) => {
                self.loading = false;
                match result {
                    Ok(spin) => {
                        self.spin = Some(spin);
                        self.selected_player = None;
                        self.selected_attribute = next_attribute(&self.picks);
                        self.error = None;
                    }
                    Err(_) => {
                        // No alternative (or request failed): refund the spent budget.
                        match kind {
                            RerollKind::Level => {
                                self.rerolls.level_used = self.rerolls.level_used.saturating_sub(1)
                            }
                            RerollKind::Tournament => {
                                self.rerolls.tournament_used =
                                    self.rerolls.tournament_used.saturating_sub(1)
                            }
                            RerollKind::Year => {
                                self.rerolls.year_used = self.rerolls.year_used.saturating_sub(1)
                            }
                        }
                        self.error = Some("Sem alternativa para essa regirada.".to_string());
                    }
                }
                true
            }
            Msg::SelectAttribute(attribute) => {
                if !self.picks.iter().any(|pick| pick.attribute == attribute) {
                    self.selected_attribute = Some(attribute);
                }
                true
            }
            Msg::SelectPlayer(player) => {
                // A player can only contribute ONE attribute to the build.
                if !self.picks.iter().any(|pick| pick.player == player.name) {
                    self.selected_player = Some(player);
                }
                true
            }
            Msg::ConfirmPick => {
                if let (Some(spin), Some(attribute), Some(player)) = (
                    self.spin.clone(),
                    self.selected_attribute,
                    self.selected_player.clone(),
                ) {
                    let rating = player.ratings.get(attribute);
                    self.picks.push(AttributePickDto {
                        attribute,
                        player: player.name,
                        rating,
                        source: spin,
                    });
                    self.spin = None;
                    self.selected_player = None;
                    self.selected_attribute = next_attribute(&self.picks);
                    if self.picks.len() < Attribute::ALL.len() {
                        ctx.link().send_message(Msg::NewSpin);
                    }
                }
                true
            }
            // Simulate the next slam (one at a time, advanced by the button).
            Msg::SimulateSlam => {
                if self.current_slam < SLAMS.len() && !self.revealing {
                    self.loading = true;
                    let (code, _) = SLAMS[self.current_slam];
                    let picks = self.picks.clone();
                    let link = ctx.link().clone();
                    spawn_local(async move {
                        let result = api::slam_edition(code)
                            .await
                            .map(|edition| sim::simulate_slam(&picks, &edition));
                        link.send_message(Msg::SlamSimulated(result));
                    });
                }
                true
            }
            Msg::SlamSimulated(result) => {
                self.loading = false;
                match result {
                    Ok(slam) => {
                        self.sim_results.push(slam);
                        self.revealed_rounds = 0;
                        self.revealing = true;
                        self.error = None;
                        schedule_reveal(ctx);
                    }
                    Err(error) => self.error = Some(error),
                }
                true
            }
            // Reveal the current slam's rounds one at a time.
            Msg::RevealRound => {
                let len = self
                    .sim_results
                    .last()
                    .map(|slam| slam.rounds.len())
                    .unwrap_or(0);
                if self.revealed_rounds < len {
                    self.revealed_rounds += 1;
                }
                if self.revealed_rounds < len {
                    schedule_reveal(ctx);
                } else {
                    self.revealing = false;
                    self.current_slam += 1;
                }
                true
            }
            Msg::NicknameChanged(value) => {
                self.nickname = value;
                true
            }
            Msg::SaveRun => {
                let run = RunDto {
                    nickname: self.nickname.trim().to_string(),
                    overall: sim::overall(&self.picks),
                    slams_won: self.sim_results.iter().filter(|result| result.won).count() as u8,
                    points: calendar_slam_shared::run_points(&self.sim_results),
                    attributes: self.picks.clone(),
                    sources: self.sim_results.clone(),
                };
                let link = ctx.link().clone();
                spawn_local(async move {
                    let result = api::save_run(&run).await.map(|_| ());
                    link.send_message(Msg::RunSaved(result));
                });
                true
            }
            Msg::DownloadImage => {
                if let Some(canvas) = self.share_canvas.cast::<HtmlCanvasElement>() {
                    let summary = share::RunSummary {
                        nickname: &self.nickname,
                        picks: &self.picks,
                        slams: &self.sim_results,
                        overall: sim::overall(&self.picks),
                        language: self.share_language(),
                    };
                    if let Err(error) = share::draw_summary(&canvas, &summary)
                        .and_then(|_| share::download_png(&canvas))
                    {
                        self.error = Some(format!("{}: {error:?}", self.text("download_error")));
                    }
                }
                true
            }
            Msg::ShareImage => {
                if let Some(canvas) = self.share_canvas.cast::<HtmlCanvasElement>() {
                    let summary = share::RunSummary {
                        nickname: &self.nickname,
                        picks: &self.picks,
                        slams: &self.sim_results,
                        overall: sim::overall(&self.picks),
                        language: self.share_language(),
                    };
                    if let Err(error) = share::draw_summary(&canvas, &summary)
                        .and_then(|_| share::share_png(&canvas))
                    {
                        self.error =
                            Some(format!("{}: {error:?}", self.text("share_error")));
                    }
                }
                true
            }
            Msg::RunSaved(result) => {
                match result {
                    Ok(()) => {
                        self.saved = true;
                        let link = ctx.link().clone();
                        spawn_local(async move {
                            link.send_message(Msg::LeaderboardLoaded(api::leaderboard().await));
                        });
                    }
                    Err(error) => self.error = Some(error),
                }
                true
            }
            Msg::LeaderboardLoaded(result) => {
                match result {
                    Ok(rows) => self.leaderboard = rows,
                    Err(error) => self.error = Some(error),
                }
                true
            }
            // Expand/collapse a ranking row to reveal which player gave each attribute.
            Msg::ToggleRun(id) => {
                if self.open_run == Some(id) {
                    self.open_run = None;
                    self.run_detail = None;
                } else {
                    self.open_run = Some(id);
                    self.run_detail = None;
                    let link = ctx.link().clone();
                    spawn_local(async move {
                        link.send_message(Msg::RunDetailLoaded(api::run_detail(id).await));
                    });
                }
                true
            }
            Msg::RunDetailLoaded(result) => {
                match result {
                    Ok(run) => self.run_detail = Some(run),
                    Err(error) => self.error = Some(error),
                }
                true
            }
            Msg::OpenMpLobby => {
                self.view = View::MpLobby;
                self.error = None;
                true
            }
            Msg::MpNameChanged(value) => {
                self.mp_name = value;
                true
            }
            Msg::MpCodeChanged(value) => {
                self.mp_join_code = value.to_ascii_uppercase();
                true
            }
            Msg::MpBracketChanged(size) => {
                self.mp_bracket_size = if size == 16 { 16 } else { 8 };
                true
            }
            Msg::MpCreateRoom => {
                self.ensure_mp_connection(ctx);
                if let Some(connection) = &self.mp_connection {
                    if let Err(error) = connection.send(MpClientMsg::CreateRoom {
                        name: self.mp_name.clone(),
                        bracket_size: self.mp_bracket_size,
                    }) {
                        self.error = Some(error);
                    }
                }
                true
            }
            Msg::MpJoinRoom => {
                self.ensure_mp_connection(ctx);
                if let Some(connection) = &self.mp_connection {
                    if let Err(error) = connection.send(MpClientMsg::JoinRoom {
                        code: self.mp_join_code.clone(),
                        name: self.mp_name.clone(),
                    }) {
                        self.error = Some(error);
                    }
                }
                true
            }
            Msg::MpStartGame => {
                if let Some(connection) = &self.mp_connection {
                    if let Err(error) = connection.send(MpClientMsg::StartGame) {
                        self.error = Some(error);
                    }
                }
                true
            }
            Msg::MpReceived(gen, result) => {
                // Drop anything from a connection we've already left.
                if gen != self.mp_gen {
                    return false;
                }
                match result {
                    Ok(message) => self.apply_mp_message(ctx, message),
                    Err(error) => self.error = Some(error),
                }
                true
            }
            Msg::MpSelectAttribute(attribute) => {
                if self
                    .my_mp_team()
                    .map(|team| !team.picks.iter().any(|pick| pick.attribute == attribute))
                    .unwrap_or(false)
                {
                    self.mp_selected_attribute = Some(attribute);
                }
                true
            }
            Msg::MpSelectPlayer(player) => {
                self.mp_selected_player = Some(player);
                true
            }
            Msg::MpConfirmPick => {
                if let (Some(connection), Some(attribute), Some(player)) = (
                    &self.mp_connection,
                    self.mp_selected_attribute,
                    self.mp_selected_player.clone(),
                ) {
                    if let Err(error) = connection.send(MpClientMsg::MakePick {
                        attribute,
                        player: player.name,
                    }) {
                        self.error = Some(error);
                    }
                }
                true
            }
            // One tick of the autopick countdown; ignore stale ticks from past turns.
            Msg::MpTick(seq) => {
                if seq != self.mp_turn_seq || self.view != View::MpDraft {
                    return false;
                }
                if self.mp_remaining > 0 {
                    self.mp_remaining -= 1;
                }
                if self.mp_remaining > 0 {
                    schedule_tick(ctx, seq);
                }
                true
            }
            // "Próximo jogo" (host only): ask the server to advance; it broadcasts to everyone.
            Msg::MpRevealNext => {
                if let Some(connection) = &self.mp_connection {
                    if let Err(error) = connection.send(MpClientMsg::RevealNext) {
                        self.error = Some(error);
                    }
                }
                true
            }
            Msg::MpRequestLeave => {
                self.mp_confirm_leave = true;
                true
            }
            Msg::MpCancelLeave => {
                self.mp_confirm_leave = false;
                true
            }
            // Click a revealed match to replay its game-by-game confrontation.
            Msg::MpSelectMatch(index) => {
                if index < self.mp_reveal {
                    self.start_match_anim(ctx, index);
                }
                true
            }
            // One game tick of the open match's score climb (1-0, 1-1, 2-1, ...).
            Msg::MpGameTick(seq) => {
                if seq != self.mp_anim_seq || self.view != View::MpKnockout {
                    return false;
                }
                if self.mp_game_shown < self.mp_game_seq.len() {
                    self.mp_game_shown += 1;
                }
                if self.mp_game_shown < self.mp_game_seq.len() {
                    schedule_game_tick(ctx, seq);
                } else {
                    // Animation finished: if this was the last match, the champion is now decided.
                    let total = self
                        .mp_bracket
                        .as_ref()
                        .map(|b| b.rounds.iter().map(|r| r.len()).sum::<usize>())
                        .unwrap_or(0);
                    if self.mp_reveal >= total {
                        self.mp_finished = true;
                    }
                }
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        match self.view {
            View::Home => self.view_home(ctx),
            View::Game => self.view_game(ctx),
            View::MpLobby => self.view_mp_lobby(ctx),
            View::MpRoom => self.view_mp_room(ctx),
            View::MpDraft => self.view_mp_draft(ctx),
            View::MpKnockout => self.view_mp_knockout(ctx),
        }
    }
}

// How-it-works steps shown on the home page (Brazilian Portuguese).
const TUTORIAL_PT: [(&str, &str); 6] = [
    (
        "Gire a roleta",
        "Cada giro sorteia uma edição real de torneio ATP: nível, torneio, ano e piso.",
    ),
    (
        "Escolha 1 atributo de 1 jogador",
        "São 8 atributos no total. Cada jogador real entrega apenas UM atributo para o seu tenista.",
    ),
    (
        "Use as regiradas",
        "Você tem regiradas limitadas (1 de nível, 2 de torneio, 2 de ano) para caçar jogadores melhores.",
    ),
    (
        "Ratings vêm do feito real",
        "Quanto mais longe o jogador foi naquela edição e mais forte o nível, maior o rating que você herda.",
    ),
    (
        "Simule os 4 Grand Slams",
        "Com o tenista montado, dispute Australian Open, Roland Garros, Wimbledon e US Open, um de cada vez.",
    ),
    (
        "Pontue e suba no ranking",
        "Você ganha pontos ATP pela colocação em cada Slam. A soma define sua posição no ranking geral.",
    ),
];

// ATP points by finishing position, shown as a reference table on the home page.
const POINTS_TABLE: [(&str, u16); 8] = [
    ("Campeão", 2000),
    ("Final", 1200),
    ("Semifinal", 720),
    ("Quartas", 360),
    ("4ª rodada", 180),
    ("3ª rodada", 90),
    ("2ª rodada", 45),
    ("1ª rodada", 10),
];

const TUTORIAL_EN: [(&str, &str); 6] = [
    (
        "Spin the draw",
        "Each spin pulls a real ATP tournament edition: level, tournament, year, and surface.",
    ),
    (
        "Pick 1 attribute from 1 player",
        "There are 8 attributes total. Each real player can give only ONE attribute to your build.",
    ),
    (
        "Use rerolls",
        "You have limited rerolls (1 level, 2 tournament, 2 year) to hunt for better players.",
    ),
    (
        "Ratings come from real runs",
        "The deeper the player went and the stronger the event level, the higher the inherited rating.",
    ),
    (
        "Simulate the 4 Grand Slams",
        "Once your player is built, play the Australian Open, Roland Garros, Wimbledon, and US Open one by one.",
    ),
    (
        "Score points and climb",
        "You earn ATP points for each Slam finish. The total defines your leaderboard position.",
    ),
];

const POINTS_TABLE_EN: [(&str, u16); 8] = [
    ("Champion", 2000),
    ("Final", 1200),
    ("Semifinal", 720),
    ("Quarterfinal", 360),
    ("Round 4", 180),
    ("Round 3", 90),
    ("Round 2", 45),
    ("Round 1", 10),
];

impl Model {
    fn text(&self, key: &'static str) -> &'static str {
        match self.language {
            Language::Pt => match key {
                "title" => "Monte o tenista perfeito",
                "hero_lead" => "Gire a roleta de torneios reais do ATP, roube um atributo de cada lenda e leve o seu Frankenstein das quadras para conquistar os quatro Grand Slams.",
                "play" => "Jogar",
                "multiplayer" => "Multiplayer",
                "how_it_works" => "Como funciona",
                "steps" => "6 passos",
                "points_by_finish" => "Pontos por colocacao",
                "per_slam" => "por Slam",
                "points_note" => "Maximo de 8000 pontos somando os quatro Grand Slams. Empate e decidido pelo overall do tenista.",
                "home" => "Inicio",
                "attributes" => "Atributos",
                "waiting_pick" => "aguardando pick",
                "roulette" => "Roleta",
                "spinning" => "girando",
                "ready" => "pronta",
                "level" => "Nivel",
                "tournament" => "Torneio",
                "year" => "Ano",
                "surface" => "Piso",
                "change_level" => "Mudar nivel",
                "change_tournament" => "Mudar torneio",
                "change_year" => "Mudar ano",
                "used" => "ja usado",
                "confirm_attribute" => "Confirmar atributo",
                "draft_complete" => "Draft completo. Simule os Slams.",
                "loading_spin" => "Carregando sorteio...",
                "titles" => "titulos",
                "won_against" => "ganhou de",
                "lost_to" => "perdeu para",
                "champion" => "Campeao!",
                "eliminated_in" => "Eliminado na",
                "simulating" => "Simulando...",
                "simulate" => "Simular",
                "next" => "Proximo",
                "run_saved" => "Run salva!",
                "ranked_points" => "pontos ATP no ranking.",
                "view_ranking" => "Ver ranking",
                "nickname" => "apelido",
                "save_run" => "Salvar run",
                "download_image" => "Baixar imagem",
                "share" => "Compartilhar",
                "best_round" => "melhor rodada",
                "leaderboard" => "Ranking geral",
                "leaderboard_hint" => "top 50 - clique p/ ver o time",
                "empty_rank" => "Nenhuma run salva ainda. Seja o primeiro!",
                "loading_team" => "Carregando time...",
                "download_error" => "Nao foi possivel baixar a imagem",
                "share_error" => "Nao foi possivel compartilhar a imagem",
                "max" => "Max",
                "create_room" => "Criar sala",
                "bots_fill" => "bots completam",
                "participants" => "Participantes",
                "join_by_code" => "Entrar por codigo",
                "separate_pcs" => "PCs separados",
                "code" => "Codigo",
                "join" => "Entrar",
                "name" => "Nome",
                "room" => "Sala",
                "bracket" => "Chave",
                "players" => "Jogadores",
                "start" => "Iniciar",
                "waiting_host" => "Aguardando o host iniciar.",
                "waiting" => "Aguardando",
                "teams" => "Times",
                "turn" => "vez",
                "your_turn" => "Sua vez",
                "turn_draw" => "Roleta da vez",
                "watching" => "assistindo",
                "waiting_next_spin" => "Aguardando proximo sorteio...",
                "confirm_pick" => "Confirmar pick",
                "final_bracket" => "Chave final",
                "result" => "resultado",
                "autopick_in" => "auto em",
                "console" => "Console do draft",
                "console_empty" => "As escolhas aparecem aqui.",
                "matches" => "Jogos",
                "strength" => "forca",
                "next_match" => "Proximo jogo",
                "click_match" => "clique num jogo",
                "click_match_hint" => "Clique em \"Proximo jogo\" para revelar os confrontos.",
                "live" => "ao vivo",
                "final_label" => "final",
                "host_controls" => "O host controla os jogos.",
                "knockout_waiting" => "Aguardando o host iniciar os jogos...",
                "leave_title" => "Sair da partida?",
                "leave_body" => "Nao tem volta: voce sai da sala e seu time vira bot.",
                "leave_cancel" => "Ficar",
                "leave_confirm" => "Sair",
                _ => key,
            },
            Language::En => match key {
                "title" => "Build the perfect tennis player",
                "hero_lead" => "Spin real ATP tournament draws, steal one attribute from each legend, and take your court-built monster through the four Grand Slams.",
                "play" => "Play",
                "multiplayer" => "Multiplayer",
                "how_it_works" => "How it works",
                "steps" => "6 steps",
                "points_by_finish" => "Points by finish",
                "per_slam" => "per Slam",
                "points_note" => "Maximum of 8000 points across the four Grand Slams. Ties are decided by player overall.",
                "home" => "Home",
                "attributes" => "Attributes",
                "waiting_pick" => "waiting for pick",
                "roulette" => "Draw",
                "spinning" => "spinning",
                "ready" => "ready",
                "level" => "Level",
                "tournament" => "Tournament",
                "year" => "Year",
                "surface" => "Surface",
                "change_level" => "Change level",
                "change_tournament" => "Change tournament",
                "change_year" => "Change year",
                "used" => "used",
                "confirm_attribute" => "Confirm attribute",
                "draft_complete" => "Draft complete. Simulate the Slams.",
                "loading_spin" => "Loading draw...",
                "titles" => "titles",
                "won_against" => "beat",
                "lost_to" => "lost to",
                "champion" => "Champion!",
                "eliminated_in" => "Eliminated in",
                "simulating" => "Simulating...",
                "simulate" => "Simulate",
                "next" => "Next",
                "run_saved" => "Run saved!",
                "ranked_points" => "ATP points on the leaderboard.",
                "view_ranking" => "View leaderboard",
                "nickname" => "nickname",
                "save_run" => "Save run",
                "download_image" => "Download image",
                "share" => "Share",
                "best_round" => "best round",
                "leaderboard" => "Overall leaderboard",
                "leaderboard_hint" => "top 50 - click to view the team",
                "empty_rank" => "No saved runs yet. Be the first!",
                "loading_team" => "Loading team...",
                "download_error" => "Could not download the image",
                "share_error" => "Could not share the image",
                "max" => "Max",
                "create_room" => "Create room",
                "bots_fill" => "bots fill",
                "participants" => "Participants",
                "join_by_code" => "Join by code",
                "separate_pcs" => "separate PCs",
                "code" => "Code",
                "join" => "Join",
                "name" => "Name",
                "room" => "Room",
                "bracket" => "Bracket",
                "players" => "Players",
                "start" => "Start",
                "waiting_host" => "Waiting for the host to start.",
                "waiting" => "Waiting",
                "teams" => "Teams",
                "turn" => "turn",
                "your_turn" => "Your turn",
                "turn_draw" => "Current draw",
                "watching" => "watching",
                "waiting_next_spin" => "Waiting for the next draw...",
                "confirm_pick" => "Confirm pick",
                "final_bracket" => "Final bracket",
                "result" => "result",
                "autopick_in" => "auto in",
                "console" => "Draft console",
                "console_empty" => "Picks show up here.",
                "matches" => "Matches",
                "strength" => "strength",
                "next_match" => "Next match",
                "click_match" => "click a match",
                "click_match_hint" => "Click \"Next match\" to reveal the matchups.",
                "live" => "live",
                "final_label" => "final",
                "host_controls" => "The host runs the matches.",
                "knockout_waiting" => "Waiting for the host to start the matches...",
                "leave_title" => "Leave the game?",
                "leave_body" => "No going back: you leave the room and your team becomes a bot.",
                "leave_cancel" => "Stay",
                "leave_confirm" => "Leave",
                _ => key,
            },
        }
    }

    fn tutorial(&self) -> &'static [(&'static str, &'static str); 6] {
        match self.language {
            Language::Pt => &TUTORIAL_PT,
            Language::En => &TUTORIAL_EN,
        }
    }

    fn points_table(&self) -> &'static [(&'static str, u16); 8] {
        match self.language {
            Language::Pt => &POINTS_TABLE,
            Language::En => &POINTS_TABLE_EN,
        }
    }

    fn language_toggle(&self, ctx: &Context<Self>) -> Html {
        html! {
            <button class="lang-toggle" onclick={ctx.link().callback(|_| Msg::ToggleLanguage)}>
                {self.language.other().code()}
            </button>
        }
    }

    fn share_language(&self) -> share::ShareLanguage {
        match self.language {
            Language::Pt => share::ShareLanguage::Pt,
            Language::En => share::ShareLanguage::En,
        }
    }

    fn ensure_mp_connection(&mut self, ctx: &Context<Self>) {
        if self.mp_connection.is_some() {
            return;
        }
        // Fresh connection: bump the generation so late messages from any old socket are ignored.
        self.mp_gen = self.mp_gen.wrapping_add(1);
        let gen = self.mp_gen;
        let callback = ctx.link().callback(move |result| Msg::MpReceived(gen, result));
        match mp::MpConnection::connect(callback) {
            Ok(connection) => self.mp_connection = Some(connection),
            Err(error) => self.error = Some(error),
        }
    }

    /// Leave any active multiplayer session: close the socket and wipe all MP state so a later
    /// join starts clean and no stale messages drive the view.
    fn leave_multiplayer(&mut self) {
        // Bump the generation first so in-flight messages from the dropped socket are ignored.
        self.mp_gen = self.mp_gen.wrapping_add(1);
        self.mp_connection = None; // dropping closes the WebSocket (server removes us)
        self.mp_your_id = None;
        self.mp_code = None;
        self.mp_host_id = None;
        self.mp_join_code.clear();
        self.mp_players.clear();
        self.mp_teams.clear();
        self.mp_spin = None;
        self.mp_on_clock = None;
        self.mp_your_turn = false;
        self.mp_picks_made = 0;
        self.mp_total_picks = 0;
        self.mp_selected_attribute = None;
        self.mp_selected_player = None;
        self.mp_bracket = None;
        self.mp_champion = None;
        self.mp_log.clear();
        self.mp_remaining = 0;
        self.mp_reveal = 0;
        self.mp_selected_match = None;
        self.mp_game_seq.clear();
        self.mp_game_shown = 0;
        self.mp_finished = false;
        self.mp_confirm_leave = false;
    }

    fn apply_mp_message(&mut self, ctx: &Context<Self>, message: MpServerMsg) {
        match message {
            MpServerMsg::Joined { your_id, code } => {
                self.mp_your_id = Some(your_id);
                self.mp_code = Some(code);
                self.view = View::MpRoom;
                self.error = None;
                self.mp_log.clear();
                self.mp_reveal = 0;
                self.mp_bracket = None;
                self.mp_champion = None;
            }
            MpServerMsg::RoomState {
                code,
                host_id,
                bracket_size,
                players,
            } => {
                self.mp_code = Some(code);
                self.mp_host_id = Some(host_id);
                self.mp_bracket_size = bracket_size;
                self.mp_players = players.clone();
                // Rebuild the team list to match the current roster (so everyone — not just the
                // last to join — sees every team), preserving any picks already made.
                self.mp_teams = players
                    .into_iter()
                    .map(|player| {
                        let picks = self
                            .mp_teams
                            .iter()
                            .find(|team| team.id == player.id)
                            .map(|team| team.picks.clone())
                            .unwrap_or_default();
                        MpTeam {
                            id: player.id,
                            name: player.name,
                            is_bot: player.is_bot,
                            picks,
                        }
                    })
                    .collect();
            }
            MpServerMsg::DraftTurn {
                on_clock,
                your_turn,
                spin,
                deadline_ms,
                picks_made,
                total_picks,
            } => {
                if self.mp_teams.is_empty() {
                    self.mp_teams = self
                        .mp_players
                        .iter()
                        .map(|player| MpTeam {
                            id: player.id.clone(),
                            name: player.name.clone(),
                            is_bot: player.is_bot,
                            picks: Vec::new(),
                        })
                        .collect();
                }
                self.view = View::MpDraft;
                self.mp_on_clock = Some(on_clock);
                self.mp_your_turn = your_turn;
                self.mp_spin = Some(spin);
                self.mp_picks_made = picks_made;
                self.mp_total_picks = total_picks;
                self.mp_selected_attribute = self.my_next_mp_attribute();
                self.mp_selected_player = None;
                // (Re)start the autopick countdown for this turn.
                self.mp_turn_seq = self.mp_turn_seq.wrapping_add(1);
                self.mp_remaining = (deadline_ms / 1000).max(1);
                schedule_tick(ctx, self.mp_turn_seq);
            }
            MpServerMsg::PickMade {
                team_id,
                attribute,
                player,
                rating,
            } => {
                let source = self.mp_spin.clone().unwrap_or_else(empty_spin);
                let team_name = self.mp_team_name(&team_id);
                if let Some(team) = self.mp_teams.iter_mut().find(|team| team.id == team_id) {
                    if !team.picks.iter().any(|pick| pick.attribute == attribute) {
                        team.picks.push(AttributePickDto {
                            attribute,
                            player: player.clone(),
                            rating,
                            source,
                        });
                    }
                }
                // Draft console line: who grabbed whom.
                self.mp_log.push(format!(
                    "{team_name} · {player} → {} ({rating})",
                    self.language.attr(attribute)
                ));
                self.mp_spin = None;
                self.mp_selected_player = None;
            }
            MpServerMsg::KnockoutResult {
                bracket,
                champion,
                teams,
            } => {
                // Prefer the server's authoritative teams (picks include their source edition).
                if !teams.is_empty() {
                    self.mp_teams = teams;
                }
                self.view = View::MpKnockout;
                self.mp_bracket = Some(bracket);
                self.mp_champion = Some(champion);
                self.mp_spin = None;
                self.mp_your_turn = false;
                self.mp_turn_seq = self.mp_turn_seq.wrapping_add(1); // stop any countdown
                self.mp_reveal = 0;
                self.mp_selected_match = None;
                self.mp_game_seq.clear();
                self.mp_game_shown = 0;
                self.mp_finished = false;
            }
            // Host advanced the knockout reveal: sync our reveal count and play the new match.
            MpServerMsg::RevealAdvance { reveal } => {
                let reveal = reveal as usize;
                let total = self
                    .mp_bracket
                    .as_ref()
                    .map(|bracket| bracket.rounds.iter().map(|round| round.len()).sum::<usize>())
                    .unwrap_or(0);
                if reveal >= 1 && reveal <= total && reveal > self.mp_reveal {
                    self.mp_reveal = reveal;
                    self.start_match_anim(ctx, reveal - 1);
                }
            }
            MpServerMsg::Error { message } => self.error = Some(message),
        }
    }

    fn my_mp_team(&self) -> Option<&MpTeam> {
        let id = self.mp_your_id.as_ref()?;
        self.mp_teams.iter().find(|team| &team.id == id)
    }

    fn my_next_mp_attribute(&self) -> Option<Attribute> {
        let team = self.my_mp_team()?;
        Attribute::ALL
            .into_iter()
            .find(|attribute| !team.picks.iter().any(|pick| pick.attribute == *attribute))
    }

    fn view_home(&self, ctx: &Context<Self>) -> Html {
        html! {
            <main class="app-shell">
                <section class="hero">
                    { self.language_toggle(ctx) }
                    <p class="eyebrow">{"CalendarSlam"}</p>
                    <h1>{self.text("title")}</h1>
                    <p class="hero-lead">
                        {self.text("hero_lead")}
                    </p>
                    <div class="hero-actions">
                        <button class="primary hero-cta" onclick={ctx.link().callback(|_| Msg::StartGame)}>
                            {self.text("play")}
                        </button>
                        <button class="hero-cta secondary-cta" onclick={ctx.link().callback(|_| Msg::OpenMpLobby)}>
                            {self.text("multiplayer")}
                        </button>
                    </div>
                </section>

                <section class="panel home-panel">
                    <div class="panel-head"><h2>{self.text("how_it_works")}</h2><span>{self.text("steps")}</span></div>
                    <ol class="tutorial">
                        { for self.tutorial().iter().enumerate().map(|(index, (title, body))| html! {
                            <li class="tutorial-step">
                                <span class="step-num">{index + 1}</span>
                                <div>
                                    <strong>{*title}</strong>
                                    <small>{*body}</small>
                                </div>
                            </li>
                        }) }
                    </ol>
                </section>

                <section class="panel home-panel">
                    <div class="panel-head"><h2>{self.text("points_by_finish")}</h2><span>{self.text("per_slam")}</span></div>
                    <div class="points-table">
                        { for self.points_table().iter().map(|(label, points)| html! {
                            <div class="points-row">
                                <span>{*label}</span>
                                <strong>{points}</strong>
                            </div>
                        }) }
                    </div>
                    <p class="points-note">
                        {self.text("points_note")}
                    </p>
                </section>

                { self.view_leaderboard(ctx) }
            </main>
        }
    }

    fn view_game(&self, ctx: &Context<Self>) -> Html {
        html! {
            <main class="app-shell">
                <section class="scoreboard">
                    <div>
                        <button class="back-link" onclick={ctx.link().callback(|_| Msg::GoHome)}>{format!("<- {}", self.text("home"))}</button>
                        { self.language_toggle(ctx) }
                        <p class="eyebrow">{"CalendarSlam"}</p>
                        <h1>{self.text("title")}</h1>
                    </div>
                    <div class="score-chip">
                        <span>{"Overall"}</span>
                        <strong>{sim::overall(&self.picks)}</strong>
                    </div>
                </section>

                { self.view_error() }

                <section class="draft-layout">
                    { self.view_attributes(ctx) }
                    { self.view_spin(ctx) }
                </section>

                { self.view_simulation(ctx) }
            </main>
        }
    }

    fn view_mp_lobby(&self, ctx: &Context<Self>) -> Html {
        html! {
            <main class="app-shell">
                <section class="scoreboard mp-hero">
                    <div>
                        <button class="back-link" onclick={ctx.link().callback(|_| Msg::GoHome)}>{format!("<- {}", self.text("home"))}</button>
                        { self.language_toggle(ctx) }
                        <p class="eyebrow">{"Multiplayer"}</p>
                        <h1>{"Knockout online"}</h1>
                    </div>
                    <div class="score-chip">
                        <span>{self.text("max")}</span>
                        <strong>{"16"}</strong>
                    </div>
                </section>
                { self.view_error() }
                <section class="mp-lobby-grid">
                    <div class="panel">
                        <div class="panel-head"><h2>{self.text("create_room")}</h2><span>{self.text("bots_fill")}</span></div>
                        <div class="mp-form">
                            { self.view_mp_name_input(ctx) }
                            <label class="mp-field">
                                <span>{self.text("participants")}</span>
                                <select onchange={ctx.link().callback(|event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    Msg::MpBracketChanged(input.value().parse().unwrap_or(8))
                                })}>
                                    <option value="8" selected={self.mp_bracket_size == 8}>{"8"}</option>
                                    <option value="16" selected={self.mp_bracket_size == 16}>{"16"}</option>
                                </select>
                            </label>
                            <button class="primary" disabled={self.mp_name.trim().is_empty()} onclick={ctx.link().callback(|_| Msg::MpCreateRoom)}>
                                {self.text("create_room")}
                            </button>
                        </div>
                    </div>
                    <div class="panel">
                        <div class="panel-head"><h2>{self.text("join_by_code")}</h2><span>{self.text("separate_pcs")}</span></div>
                        <div class="mp-form">
                            { self.view_mp_name_input(ctx) }
                            <label class="mp-field">
                                <span>{self.text("code")}</span>
                                <input
                                    value={self.mp_join_code.clone()}
                                    maxlength="6"
                                    oninput={ctx.link().callback(|event: InputEvent| {
                                        let input: HtmlInputElement = event.target_unchecked_into();
                                        Msg::MpCodeChanged(input.value())
                                    })}
                                />
                            </label>
                            <button class="primary" disabled={self.mp_name.trim().is_empty() || self.mp_join_code.trim().is_empty()} onclick={ctx.link().callback(|_| Msg::MpJoinRoom)}>
                                {self.text("join")}
                            </button>
                        </div>
                    </div>
                </section>
            </main>
        }
    }

    fn view_mp_name_input(&self, ctx: &Context<Self>) -> Html {
        html! {
            <label class="mp-field">
                <span>{self.text("name")}</span>
                <input
                    value={self.mp_name.clone()}
                    maxlength="24"
                    oninput={ctx.link().callback(|event: InputEvent| {
                        let input: HtmlInputElement = event.target_unchecked_into();
                        Msg::MpNameChanged(input.value())
                    })}
                />
            </label>
        }
    }

    fn view_mp_room(&self, ctx: &Context<Self>) -> Html {
        let is_host = self.mp_your_id.as_deref() == self.mp_host_id.as_deref();
        html! {
            <main class="app-shell">
                <section class="scoreboard mp-hero">
                    <div>
                        <button class="back-link" onclick={ctx.link().callback(|_| Msg::MpRequestLeave)}>{format!("<- {}", self.text("home"))}</button>
                        { self.language_toggle(ctx) }
                        <p class="eyebrow">{self.text("room")}</p>
                        <h1>{self.mp_code.clone().unwrap_or_else(|| "-----".to_string())}</h1>
                    </div>
                    <div class="score-chip"><span>{self.text("bracket")}</span><strong>{self.mp_bracket_size}</strong></div>
                </section>
                { self.view_error() }
                <section class="panel mp-room">
                    <div class="panel-head">
                        <h2>{self.text("players")}</h2>
                        <span>{format!("{}/{}", self.mp_players.len(), self.mp_bracket_size)}</span>
                    </div>
                    <div class="mp-player-grid">
                        { for self.mp_players.iter().map(|player| html! {
                            <div class={classes!("mp-player-card", player.is_bot.then_some("bot"), (!player.connected).then_some("offline"))}>
                                <strong>{&player.name}</strong>
                                <small>{ if player.is_bot { "bot" } else if player.connected { "online" } else { "offline" } }</small>
                            </div>
                        }) }
                    </div>
                    {
                        if is_host {
                            html! { <button class="primary" onclick={ctx.link().callback(|_| Msg::MpStartGame)}>{self.text("start")}</button> }
                        } else {
                            html! { <div class="complete-card">{self.text("waiting_host")}</div> }
                        }
                    }
                </section>
                { self.view_leave_modal(ctx) }
            </main>
        }
    }

    fn view_mp_draft(&self, ctx: &Context<Self>) -> Html {
        let on_clock_name = self
            .mp_on_clock
            .as_ref()
            .and_then(|id| self.mp_teams.iter().find(|team| &team.id == id))
            .map(|team| team.name.clone())
            .unwrap_or_else(|| self.text("waiting").to_string());
        html! {
            <main class="app-shell">
                <section class="scoreboard mp-hero">
                    <div>
                        <button class="back-link" onclick={ctx.link().callback(|_| Msg::MpRequestLeave)}>{format!("<- {}", self.text("home"))}</button>
                        { self.language_toggle(ctx) }
                        <p class="eyebrow">{format!("Sala {}", self.mp_code.clone().unwrap_or_default())}</p>
                        <h1>{"Draft snake"}</h1>
                    </div>
                    <div class="score-chip"><span>{"Pick"}</span><strong>{format!("{}/{}", self.mp_picks_made, self.mp_total_picks)}</strong></div>
                </section>
                { self.view_error() }
                <section class="mp-draft-layout">
                    <div class="panel">
                        <div class="panel-head"><h2>{self.text("teams")}</h2><span>{format!("{}: {on_clock_name}", self.text("turn"))}</span></div>
                        <div class="mp-team-list">
                            { for self.mp_teams.iter().map(|team| self.view_mp_team(team)) }
                        </div>
                    </div>
                    <div class="panel roulette-panel">
                        <div class="panel-head">
                            <h2>{ if self.mp_your_turn { self.text("your_turn") } else { self.text("turn_draw") } }</h2>
                            <span class={classes!("mp-timer", (self.mp_remaining <= 5).then_some("urgent"))}>
                                {format!("{} {}s", self.text("autopick_in"), self.mp_remaining)}
                            </span>
                        </div>
                        { self.view_mp_spin(ctx) }
                    </div>
                </section>
                { self.view_mp_console() }
                { self.view_leave_modal(ctx) }
            </main>
        }
    }

    fn view_mp_console(&self) -> Html {
        html! {
            <section class="panel mp-console">
                <div class="panel-head"><h2>{self.text("console")}</h2><span>{format!("{}/{}", self.mp_picks_made, self.mp_total_picks)}</span></div>
                <div class="mp-console-log">
                    {
                        if self.mp_log.is_empty() {
                            html! { <small class="mp-console-empty">{self.text("console_empty")}</small> }
                        } else {
                            html! { { for self.mp_log.iter().rev().map(|line| html! { <div class="mp-log-line">{line}</div> }) } }
                        }
                    }
                </div>
            </section>
        }
    }

    fn view_mp_team(&self, team: &MpTeam) -> Html {
        html! {
            <article class={classes!("mp-team-card", (self.mp_on_clock.as_deref() == Some(team.id.as_str())).then_some("active"))}>
                <div><strong>{&team.name}</strong><small>{format!("{}/8", team.picks.len())}</small></div>
                <div class="mp-mini-picks">
                    { for Attribute::ALL.iter().map(|attribute| {
                        let pick = team.picks.iter().find(|pick| pick.attribute == *attribute);
                        html! {
                            <span class={classes!(pick.is_some().then_some("filled"))} title={self.language.attr(*attribute)}>
                                {pick.map(|pick| pick.rating.to_string()).unwrap_or_else(|| "-".to_string())}
                            </span>
                        }
                    }) }
                </div>
            </article>
        }
    }

    fn view_mp_spin(&self, ctx: &Context<Self>) -> Html {
        let Some(spin) = &self.mp_spin else {
            return html! { <div class="complete-card">{self.text("waiting_next_spin")}</div> };
        };
        // Attributes already filled on my team (can't be picked again).
        let mine: Vec<Attribute> = self
            .my_mp_team()
            .map(|team| team.picks.iter().map(|pick| pick.attribute).collect())
            .unwrap_or_default();
        html! {
            <>
                <div class="roulette-strip">
                    <div><span>{self.text("level")}</span><strong>{level_label(spin.level)}</strong></div>
                    <div><span>{self.text("tournament")}</span><strong>{&spin.tournament}</strong></div>
                    <div><span>{self.text("year")}</span><strong>{spin.year}</strong></div>
                    <div><span>{self.text("surface")}</span><strong>{self.language.surface(spin.surface)}</strong></div>
                </div>
                <div class="attribute-grid mp-attr-grid">
                    { for Attribute::ALL.iter().map(|attribute| {
                        let used = self.my_mp_team()
                            .map(|team| team.picks.iter().any(|pick| pick.attribute == *attribute))
                            .unwrap_or(true);
                        let selected = self.mp_selected_attribute == Some(*attribute);
                        let attribute_copy = *attribute;
                        html! {
                            <button class={classes!("attribute-card", selected.then_some("selected"), used.then_some("filled"))}
                                disabled={!self.mp_your_turn || used}
                                onclick={ctx.link().callback(move |_| Msg::MpSelectAttribute(attribute_copy))}>
                                <span>{self.language.attr(*attribute)}</span>
                                <strong>{if used { "OK".to_string() } else { "--".to_string() }}</strong>
                            </button>
                        }
                    }) }
                </div>
                <div class="player-list">
                    { for spin.players.iter().cloned().map(|player| self.view_mp_player_row(ctx, player)) }
                </div>
                { self.view_mp_selected_ratings(ctx, &mine) }
                <button class="primary" disabled={!self.mp_your_turn || self.mp_selected_attribute.is_none() || self.mp_selected_player.is_none()} onclick={ctx.link().callback(|_| Msg::MpConfirmPick)}>
                    {self.text("confirm_pick")}
                </button>
            </>
        }
    }

    // Simple one-line preview of a player's 8 ratings (canonical order), with the currently
    // picked attribute highlighted. Shared by the single-player and multiplayer drafts.
    fn ratings_preview(&self, player: &PlayerDto, selected: Option<Attribute>) -> Html {
        html! {
            <small class="ratings-line">
                <b class="rp-round">{&player.best_round}</b>
                { for Attribute::ALL.iter().map(|attribute| {
                    let hl = selected == Some(*attribute);
                    html! {
                        <span class={classes!("rp", hl.then_some("hl"))} title={self.language.attr(*attribute)}>
                            {player.ratings.get(*attribute)}
                        </span>
                    }
                }) }
            </small>
        }
    }

    fn view_mp_player_row(&self, ctx: &Context<Self>, player: PlayerDto) -> Html {
        let selected =
            self.mp_selected_player.as_ref().map(|p| p.name.as_str()) == Some(player.name.as_str());
        let chosen = player.clone();
        html! {
            <button class={classes!("player-row", selected.then_some("selected"))}
                disabled={!self.mp_your_turn}
                onclick={ctx.link().callback(move |_| Msg::MpSelectPlayer(chosen.clone()))}>
                <span>{&player.name}</span>
                { self.ratings_preview(&player, self.mp_selected_attribute) }
            </button>
        }
    }

    // The selected player's full ratings — each non-taken attribute is clickable to pick it.
    fn view_mp_selected_ratings(&self, ctx: &Context<Self>, mine: &[Attribute]) -> Html {
        let Some(player) = &self.mp_selected_player else {
            return Html::default();
        };
        html! {
            <div class="ratings-card">
                <div class="ratings-head">
                    <strong>{&player.name}</strong>
                    <span>{format!("{}: {}", self.text("best_round"), player.best_round)}</span>
                </div>
                <div class="ratings-grid">
                    { for Attribute::ALL.iter().map(|attribute| {
                        let selected = self.mp_selected_attribute == Some(*attribute);
                        let taken = mine.contains(attribute);
                        let attribute_copy = *attribute;
                        html! {
                            <button class={classes!("rating-pill", "rating-pick", selected.then_some("selected"), taken.then_some("used"))}
                                disabled={!self.mp_your_turn || taken}
                                onclick={ctx.link().callback(move |_| Msg::MpSelectAttribute(attribute_copy))}>
                                <span>{self.language.attr(*attribute)}</span>
                                <strong>{player.ratings.get(*attribute)}</strong>
                            </button>
                        }
                    })}
                </div>
            </div>
        }
    }

    // Open a match's head-to-head and (re)start its game-by-game score animation, using the
    // server's authoritative running scores (`games[set][game]`).
    fn start_match_anim(&mut self, ctx: &Context<Self>, index: usize) {
        self.mp_selected_match = Some(index);
        self.mp_game_seq = self
            .mp_matches_in_order()
            .get(index)
            .map(|(_, game)| {
                game.games
                    .iter()
                    .enumerate()
                    .flat_map(|(set_index, frames)| {
                        frames.iter().map(move |score| (set_index, score.a, score.b))
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.mp_game_shown = 0;
        self.mp_anim_seq = self.mp_anim_seq.wrapping_add(1);
        schedule_game_tick(ctx, self.mp_anim_seq);
    }

    // Flatten the bracket into reveal order (round by round) for the match-by-match spotlight.
    fn mp_matches_in_order(&self) -> Vec<(usize, BracketMatch)> {
        self.mp_bracket
            .as_ref()
            .map(|bracket| {
                bracket
                    .rounds
                    .iter()
                    .enumerate()
                    .flat_map(|(round_index, round)| {
                        round.iter().cloned().map(move |game| (round_index, game))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn view_mp_knockout(&self, ctx: &Context<Self>) -> Html {
        let matches = self.mp_matches_in_order();
        let total = matches.len();
        let all_revealed = self.mp_reveal >= total;
        let anim_done = self.mp_game_shown >= self.mp_game_seq.len();
        // The champion is only announced once the FINAL match has finished playing out
        // (and stays announced thereafter, even if you replay an earlier match).
        let all_done = all_revealed && (anim_done || self.mp_finished);
        let champion = if all_done {
            self.mp_champion
                .as_ref()
                .map(|id| self.mp_team_name(id))
                .unwrap_or_else(|| self.text("champion").to_string())
        } else {
            "?".to_string()
        };
        // The match whose attribute head-to-head is open (clicked or the last revealed).
        let spotlight = self
            .mp_selected_match
            .filter(|index| *index < self.mp_reveal)
            .and_then(|index| matches.get(index));

        let is_host = self.mp_your_id.as_deref() == self.mp_host_id.as_deref();
        // Don't let the host race ahead while the current match is still playing out.
        let animating = self.mp_selected_match.is_some() && !anim_done;

        html! {
            <main class="app-shell">
                <section class="scoreboard mp-hero">
                    <div>
                        <button class="back-link" onclick={ctx.link().callback(|_| Msg::MpRequestLeave)}>{format!("<- {}", self.text("home"))}</button>
                        { self.language_toggle(ctx) }
                        <p class="eyebrow">{"Knockout"}</p>
                        <h1>{champion.clone()}</h1>
                    </div>
                    <div class="score-chip"><span>{self.text("matches")}</span><strong>{format!("{}/{}", self.mp_reveal.min(total), total)}</strong></div>
                </section>
                { self.view_error() }
                <div class="mp-knockout-control">
                    {
                        if is_host {
                            html! {
                                <button class="primary" disabled={all_revealed || animating} onclick={ctx.link().callback(|_| Msg::MpRevealNext)}>
                                    {format!("{} ({}/{})", self.text("next_match"), self.mp_reveal.min(total), total)}
                                </button>
                            }
                        } else if !all_done {
                            html! { <div class="complete-card">{self.text("host_controls")}</div> }
                        } else {
                            Html::default()
                        }
                    }
                    { if all_done { html! { <div class="mp-champion-banner">{format!("🏆 {champion}")}</div> } } else { Html::default() } }
                </div>
                {
                    if let Some((round_index, game)) = spotlight {
                        self.view_mp_spotlight(*round_index, game)
                    } else if !all_revealed {
                        html! { <div class="complete-card">{self.text("knockout_waiting")}</div> }
                    } else {
                        Html::default()
                    }
                }
                <section class="panel mp-bracket">
                    <div class="panel-head"><h2>{self.text("final_bracket")}</h2><span>{self.text("click_match")}</span></div>
                    <div class="mp-bracket-rounds">
                        { self.view_mp_bracket_overview(ctx) }
                    </div>
                </section>
                { self.view_leave_modal(ctx) }
            </main>
        }
    }

    // Confirmation overlay shown before leaving an active multiplayer game (no going back).
    fn view_leave_modal(&self, ctx: &Context<Self>) -> Html {
        if !self.mp_confirm_leave {
            return Html::default();
        }
        html! {
            <div class="modal-overlay">
                <div class="modal-card">
                    <h2>{self.text("leave_title")}</h2>
                    <p>{self.text("leave_body")}</p>
                    <div class="modal-actions">
                        <button onclick={ctx.link().callback(|_| Msg::MpCancelLeave)}>{self.text("leave_cancel")}</button>
                        <button class="primary" onclick={ctx.link().callback(|_| Msg::GoHome)}>{self.text("leave_confirm")}</button>
                    </div>
                </div>
            </div>
        }
    }

    // True once a match has finished playing out (so its winner/score can be shown).
    fn mp_match_settled(&self, flat: usize) -> bool {
        flat < self.mp_reveal
            && !(self.mp_selected_match == Some(flat)
                && self.mp_game_shown < self.mp_game_seq.len())
    }

    // The bracket grid; revealed matches are clickable (open their H2H). Results stay hidden until
    // a match has finished its game-by-game animation.
    fn view_mp_bracket_overview(&self, ctx: &Context<Self>) -> Html {
        let Some(bracket) = &self.mp_bracket else {
            return Html::default();
        };
        let mut offset = 0usize;
        let mut prev_base = 0usize;
        let mut rounds_html: Vec<Html> = Vec::new();
        for (round_index, round) in bracket.rounds.iter().enumerate() {
            let base = offset;
            let games: Html = round
                .iter()
                .enumerate()
                .map(|(j, game)| {
                    let flat = base + j;
                    let revealed = flat < self.mp_reveal;
                    let settled = self.mp_match_settled(flat);
                    let open = self.mp_selected_match == Some(flat);
                    // A later-round pairing is only shown once both feeder matches are settled,
                    // so the final isn't spoiled before the semis are played.
                    let teams_known = round_index == 0
                        || (self.mp_match_settled(prev_base + j * 2)
                            && self.mp_match_settled(prev_base + j * 2 + 1));
                    let a = if teams_known { self.mp_team_name(&game.a) } else { "?".to_string() };
                    let b = if teams_known { self.mp_team_name(&game.b) } else { "?".to_string() };
                    let a_won = settled && game.winner.as_deref() == Some(game.a.as_str());
                    let b_won = settled && game.winner.as_deref() == Some(game.b.as_str());
                    let result = if settled {
                        format!("{} · {}", self.language.surface(game.surface), self.sets_label(game))
                    } else if revealed {
                        format!("{} · {}", self.language.surface(game.surface), self.text("live"))
                    } else {
                        self.language.surface(game.surface).to_string()
                    };
                    html! {
                        <button
                            class={classes!("mp-match", (!revealed).then_some("pending"), open.then_some("open"))}
                            disabled={!revealed}
                            onclick={ctx.link().callback(move |_| Msg::MpSelectMatch(flat))}>
                            <span class={classes!(a_won.then_some("win"))}>{a}</span>
                            <span class={classes!(b_won.then_some("win"))}>{b}</span>
                            <b>{result}</b>
                        </button>
                    }
                })
                .collect();
            rounds_html.push(html! {
                <div class="mp-bracket-round">
                    <strong>{round_label(round_index, self.mp_bracket_size, self.language)}</strong>
                    { games }
                </div>
            });
            prev_base = base;
            offset += round.len();
        }
        rounds_html.into_iter().collect()
    }

    // The spotlight card: live game-by-game scoreboard + full builds of both teams (with sources).
    fn view_mp_spotlight(&self, round_index: usize, game: &BracketMatch) -> Html {
        let team_a = self.mp_teams.iter().find(|team| team.id == game.a);
        let team_b = self.mp_teams.iter().find(|team| team.id == game.b);
        let done = self.mp_game_shown >= self.mp_game_seq.len();
        // Only reveal the winner once the match has finished playing out.
        let winner_a = done && game.winner.as_deref() == Some(game.a.as_str());
        let winner_b = done && game.winner.as_deref() == Some(game.b.as_str());
        html! {
            <section class="panel mp-spotlight">
                <div class="panel-head">
                    <h2>{round_label(round_index, self.mp_bracket_size, self.language)}</h2>
                    <span>{format!("{} · {}", self.language.surface(game.surface), if done { self.text("final_label") } else { self.text("live") })}</span>
                </div>
                { self.view_mp_scoreboard(game) }
                <div class="mp-h2h">
                    { self.view_mp_h2h_team(team_a, game.surface, winner_a) }
                    <div class="mp-h2h-vs"><small>{"VS"}</small></div>
                    { self.view_mp_h2h_team(team_b, game.surface, winner_b) }
                </div>
            </section>
        }
    }

    // Live set strip + the climbing current-set game score (1-0, 1-1, 2-1, 3-1, ...).
    // Only sets already reached are shown, so the number of sets isn't spoiled mid-match.
    fn view_mp_scoreboard(&self, game: &BracketMatch) -> Html {
        let shown = self.mp_game_shown;
        let done = shown >= self.mp_game_seq.len();
        let (cur_set, run_a, run_b) = if shown == 0 {
            (0, 0, 0)
        } else {
            self.mp_game_seq[shown - 1]
        };
        // Reveal completed sets plus the one in progress — never the sets still to come.
        let visible = if done {
            game.sets.len()
        } else {
            (cur_set + 1).min(game.sets.len())
        };
        html! {
            <div class="mp-scoreboard">
                <div class="mp-sets">
                    { for game.sets.iter().take(visible).enumerate().map(|(i, set)| {
                        let live = !done && i == cur_set;
                        let (a, b) = if live { (run_a, run_b) } else { (set.a, set.b) };
                        html! {
                            <div class={classes!("mp-set", live.then_some("live"))}>
                                <span>{a}</span>
                                <span>{b}</span>
                            </div>
                        }
                    }) }
                </div>
                <div class={classes!("mp-live-game", (!done).then_some("playing"))}>
                    <strong>{format!("{} - {}", run_a, run_b)}</strong>
                    <small>{ if done { self.text("final_label") } else { self.text("live") } }</small>
                </div>
            </div>
        }
    }

    fn view_mp_h2h_team(&self, team: Option<&MpTeam>, surface: Surface, won: bool) -> Html {
        let Some(team) = team else {
            return html! { <div class="mp-h2h-team">{"--"}</div> };
        };
        let strength = calendar_slam_shared::team_strength(&team.picks, surface).round() as i64;
        html! {
            <div class={classes!("mp-h2h-team", won.then_some("winner"))}>
                <div class="mp-h2h-head">
                    <strong>{&team.name}</strong>
                    <span>{format!("{}: {}", self.text("strength"), strength)}{ if won { " ✓" } else { "" } }</span>
                </div>
                <div class="mp-h2h-attrs">
                    { for Attribute::ALL.iter().map(|attribute| {
                        let pick = team.picks.iter().find(|pick| pick.attribute == *attribute);
                        html! {
                            <div class="mp-h2h-attr">
                                <span class="h2h-name">{self.language.attr(*attribute)}</span>
                                <strong class="h2h-rating">{pick.map(|p| p.rating.to_string()).unwrap_or_else(|| "-".to_string())}</strong>
                                <span class="h2h-player">{pick.map(|p| p.player.clone()).unwrap_or_default()}</span>
                                <small class="h2h-src">{pick.map(|p| format!("{} {}", p.source.tournament, p.source.year)).unwrap_or_default()}</small>
                            </div>
                        }
                    }) }
                </div>
            </div>
        }
    }

    fn sets_label(&self, game: &BracketMatch) -> String {
        if game.sets.is_empty() {
            return self.mp_team_name(game.winner.as_deref().unwrap_or_default());
        }
        game.sets
            .iter()
            .map(|set| format!("{}-{}", set.a, set.b))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn mp_team_name(&self, id: &str) -> String {
        self.mp_teams
            .iter()
            .find(|team| team.id == id)
            .map(|team| team.name.clone())
            .unwrap_or_else(|| id.to_string())
    }

    fn view_error(&self) -> Html {
        self.error
            .as_ref()
            .map(|error| html! { <div class="alert">{error}</div> })
            .unwrap_or_default()
    }

    fn view_attributes(&self, ctx: &Context<Self>) -> Html {
        html! {
            <section class="panel">
                <div class="panel-head">
                    <h2>{self.text("attributes")}</h2>
                    <span>{format!("{}/8", self.picks.len())}</span>
                </div>
                <div class="attribute-grid">
                    { for Attribute::ALL.iter().map(|attribute| {
                        let pick = self.picks.iter().find(|pick| pick.attribute == *attribute);
                        let selected = self.selected_attribute == Some(*attribute);
                        let class = classes!("attribute-card", selected.then_some("selected"), pick.is_some().then_some("filled"));
                        let attribute_copy = *attribute;
                        html! {
                            <button class={class} onclick={ctx.link().callback(move |_| Msg::SelectAttribute(attribute_copy))}>
                                <span>{self.language.attr(*attribute)}</span>
                                <strong>{pick.map(|pick| pick.rating.to_string()).unwrap_or_else(|| "--".to_string())}</strong>
                                <small>{pick.map(|pick| pick.player.clone()).unwrap_or_else(|| self.text("waiting_pick").to_string())}</small>
                            </button>
                        }
                    })}
                </div>
            </section>
        }
    }

    fn view_spin(&self, ctx: &Context<Self>) -> Html {
        let disabled = self.loading || self.picks.len() == Attribute::ALL.len();
        let level_left = LEVEL_REROLLS - self.rerolls.level_used;
        let tournament_left = TOURNAMENT_REROLLS - self.rerolls.tournament_used;
        let year_left = YEAR_REROLLS - self.rerolls.year_used;

        html! {
            <section class="panel roulette-panel">
                <div class="panel-head">
                    <h2>{self.text("roulette")}</h2>
                    <span>{ if self.loading { self.text("spinning") } else { self.text("ready") } }</span>
                </div>
                {
                    if let Some(spin) = &self.spin {
                        html! {
                            <>
                                <div class={classes!("roulette-strip", self.loading.then_some("spinning"))}>
                                    <div><span>{self.text("level")}</span><strong>{level_label(spin.level)}</strong></div>
                                    <div><span>{self.text("tournament")}</span><strong>{&spin.tournament}</strong></div>
                                    <div><span>{self.text("year")}</span><strong>{spin.year}</strong></div>
                                    <div><span>{self.text("surface")}</span><strong>{self.language.surface(spin.surface)}</strong></div>
                                </div>
                                <div class="reroll-row">
                                    <button disabled={disabled || level_left == 0} onclick={ctx.link().callback(|_| Msg::RerollLevel)}>{format!("{} ({level_left})", self.text("change_level"))}</button>
                                    <button disabled={disabled || tournament_left == 0} onclick={ctx.link().callback(|_| Msg::RerollTournament)}>{format!("{} ({tournament_left})", self.text("change_tournament"))}</button>
                                    <button disabled={disabled || year_left == 0} onclick={ctx.link().callback(|_| Msg::RerollYear)}>{format!("{} ({year_left})", self.text("change_year"))}</button>
                                </div>
                                <div class="player-list">
                                    { for spin.players.iter().cloned().map(|player| {
                                        let used = self.picks.iter().any(|pick| pick.player == player.name);
                                        let selected = self.selected_player.as_ref().map(|p| p.name.as_str()) == Some(player.name.as_str());
                                        let class = classes!("player-row", selected.then_some("selected"), used.then_some("used"));
                                        let selected_player = player.clone();
                                        html! {
                                            <button class={class} disabled={used} onclick={ctx.link().callback(move |_| Msg::SelectPlayer(selected_player.clone()))}>
                                                <span>{&player.name}</span>
                                                { if used { html! { <small>{self.text("used")}</small> } } else { self.ratings_preview(&player, self.selected_attribute) } }
                                            </button>
                                        }
                                    })}
                                </div>
                                { self.view_selected_player_ratings(ctx) }
                                <button class="primary" disabled={self.selected_player.is_none() || self.selected_attribute.is_none()} onclick={ctx.link().callback(|_| Msg::ConfirmPick)}>
                                    {self.text("confirm_attribute")}
                                </button>
                            </>
                        }
                    } else if self.picks.len() == Attribute::ALL.len() {
                        html! { <div class="complete-card">{self.text("draft_complete")}</div> }
                    } else {
                        html! { <div class="complete-card">{self.text("loading_spin")}</div> }
                    }
                }
            </section>
        }
    }

    fn view_simulation(&self, ctx: &Context<Self>) -> Html {
        if self.picks.len() < Attribute::ALL.len() {
            return Html::default();
        }

        // While a slam is still revealing round-by-round, don't count it yet — the header
        // should only reflect slams whose reveal has finished.
        let counted = if self.revealing && !self.sim_results.is_empty() {
            &self.sim_results[..self.sim_results.len() - 1]
        } else {
            &self.sim_results[..]
        };
        let titles = counted.iter().filter(|result| result.won).count();
        let points = calendar_slam_shared::run_points(counted);
        let all_done = self.current_slam >= SLAMS.len();

        html! {
            <section class="panel sim-panel">
                <div class="panel-head">
                    <h2>{"Grand Slams"}</h2>
                    <span>{format!("{}/4 {} - {} pts", titles, self.text("titles"), points)}</span>
                </div>
                <div class="slam-grid">
                    { for self.sim_results.iter().enumerate().map(|(index, result)| {
                        let is_current = index + 1 == self.sim_results.len();
                        let shown = if is_current { self.revealed_rounds } else { result.rounds.len() };
                        self.view_slam(result, shown)
                    }) }
                </div>
                { self.view_sim_control(ctx, all_done) }
                { if all_done && !self.revealing { self.view_save_run(ctx) } else { Html::default() } }
            </section>
        }
    }

    fn view_slam(&self, result: &SlamResultDto, shown: usize) -> Html {
        let finished = shown >= result.rounds.len();
        html! {
            <article class={classes!("slam-card", (finished && result.won).then_some("won"))}>
                <strong>{format!("{} {}", result.tournament, result.year)}</strong>
                <div class="round-list">
                    { for result.rounds.iter().take(shown).map(|round| html! {
                        <div class={classes!("round-line", round.won.then_some("win"), (!round.won).then_some("loss"))}>
                            <span>{&round.round}</span>
                            <small>{ if round.won {
                                format!("{} {}", self.text("won_against"), round.opponent)
                            } else {
                                format!("{} {}", self.text("lost_to"), round.opponent)
                            } }</small>
                        </div>
                    }) }
                </div>
                { if finished {
                    html! {
                        <span class="slam-outcome">
                            { if result.won { self.text("champion").to_string() } else { format!("{} {}", self.text("eliminated_in"), result.exit_round) } }
                            <em class="slam-points">{format!("+{} pts", result.points)}</em>
                        </span>
                    }
                } else {
                    Html::default()
                } }
            </article>
        }
    }

    fn view_sim_control(&self, ctx: &Context<Self>, all_done: bool) -> Html {
        if self.revealing {
            return html! { <button class="primary" disabled=true>{self.text("simulating")}</button> };
        }
        if all_done {
            return Html::default();
        }
        let name = SLAMS[self.current_slam].1;
        let label = if self.current_slam == 0 {
            format!("{} {name}", self.text("simulate"))
        } else {
            format!("{}: {name}", self.text("next"))
        };
        html! {
            <button class="primary" disabled={self.loading} onclick={ctx.link().callback(|_| Msg::SimulateSlam)}>
                {label}
            </button>
        }
    }

    fn view_save_run(&self, ctx: &Context<Self>) -> Html {
        if self.sim_results.is_empty() {
            return Html::default();
        }

        if self.saved {
            return html! {
                <div class="results-actions">
                    { self.view_share_image(ctx) }
                    <div class="save-run saved">
                        <div class="saved-msg">
                            {format!("{} {} {}", self.text("run_saved"), calendar_slam_shared::run_points(&self.sim_results), self.text("ranked_points"))}
                        </div>
                        <button class="primary" onclick={ctx.link().callback(|_| Msg::GoHome)}>
                            {self.text("view_ranking")}
                        </button>
                    </div>
                </div>
            };
        }

        html! {
            <div class="results-actions">
                { self.view_share_image(ctx) }
                <div class="save-run">
                    <input
                        placeholder={self.text("nickname")}
                        value={self.nickname.clone()}
                        oninput={ctx.link().callback(|event: InputEvent| {
                            let input: HtmlInputElement = event.target_unchecked_into();
                            Msg::NicknameChanged(input.value())
                        })}
                    />
                    <button
                        class="primary"
                        disabled={self.nickname.trim().is_empty()}
                        onclick={ctx.link().callback(|_| Msg::SaveRun)}
                    >
                        {self.text("save_run")}
                    </button>
                </div>
            </div>
        }
    }

    fn view_share_image(&self, ctx: &Context<Self>) -> Html {
        html! {
            <div class="share-run">
                <canvas
                    ref={self.share_canvas.clone()}
                    width="1080"
                    height="1350"
                    aria-hidden="true"
                ></canvas>
                <button onclick={ctx.link().callback(|_| Msg::DownloadImage)}>
                    {self.text("download_image")}
                </button>
                <button class="primary" onclick={ctx.link().callback(|_| Msg::ShareImage)}>
                    {self.text("share")}
                </button>
            </div>
        }
    }

    fn view_selected_player_ratings(&self, ctx: &Context<Self>) -> Html {
        let Some(player) = &self.selected_player else {
            return Html::default();
        };

        html! {
            <div class="ratings-card">
                <div class="ratings-head">
                    <strong>{&player.name}</strong>
                    <span>{format!("{}: {}", self.text("best_round"), player.best_round)}</span>
                </div>
                <div class="ratings-grid">
                    { for Attribute::ALL.iter().map(|attribute| {
                        let selected = self.selected_attribute == Some(*attribute);
                        let taken = self.picks.iter().any(|pick| pick.attribute == *attribute);
                        let attribute_copy = *attribute;
                        html! {
                            <button class={classes!("rating-pill", "rating-pick", selected.then_some("selected"), taken.then_some("used"))}
                                disabled={taken}
                                onclick={ctx.link().callback(move |_| Msg::SelectAttribute(attribute_copy))}>
                                <span>{self.language.attr(*attribute)}</span>
                                <strong>{player.ratings.get(*attribute)}</strong>
                            </button>
                        }
                    })}
                </div>
            </div>
        }
    }

    fn view_leaderboard(&self, ctx: &Context<Self>) -> Html {
        html! {
            <section class="panel leaderboard">
                <div class="panel-head">
                    <h2>{self.text("leaderboard")}</h2>
                    <span>{self.text("leaderboard_hint")}</span>
                </div>
                {
                    if self.leaderboard.is_empty() {
                        html! { <div class="empty-rank">{self.text("empty_rank")}</div> }
                    } else {
                        html! {
                            { for self.leaderboard.iter().enumerate().map(|(index, row)| {
                                let id = row.id;
                                let open = self.open_run == Some(id);
                                html! {
                                    <>
                                        <button class={classes!("leader-row", open.then_some("open"))}
                                            onclick={ctx.link().callback(move |_| Msg::ToggleRun(id))}>
                                            <strong>{format!("#{}", index + 1)}</strong>
                                            <span>{&row.nickname}</span>
                                            <b class="leader-points">{format!("{} pts", row.points)}</b>
                                            <small>{format!("{} Slams · OVR {} {}", row.slams_won, row.overall, if open { "▲" } else { "▼" })}</small>
                                        </button>
                                        { if open { self.view_run_build() } else { Html::default() } }
                                    </>
                                }
                            })}
                        }
                    }
                }
            </section>
        }
    }

    /// The expanded build of the open ranking run: each attribute, the player it came from,
    /// the inherited rating, and the source edition.
    fn view_run_build(&self) -> Html {
        let Some(run) = &self.run_detail else {
            return html! { <div class="run-build loading">{self.text("loading_team")}</div> };
        };
        html! {
            <div class="run-build">
                { for Attribute::ALL.iter().map(|attribute| {
                    match run.attributes.iter().find(|pick| pick.attribute == *attribute) {
                        Some(pick) => html! {
                            <div class="build-cell">
                                <span class="build-attr">{self.language.attr(*attribute)}</span>
                                <strong class="build-rating">{pick.rating}</strong>
                                <span class="build-player">{&pick.player}</span>
                                <small class="build-src">{format!("{} {}", pick.source.tournament, pick.source.year)}</small>
                            </div>
                        },
                        None => html! {
                            <div class="build-cell empty">
                                <span class="build-attr">{self.language.attr(*attribute)}</span>
                                <small>{"—"}</small>
                            </div>
                        },
                    }
                })}
            </div>
        }
    }
}

fn fetch_spin(ctx: &Context<Model>, level: Option<Level>, tournament: Option<String>) {
    let link = ctx.link().clone();
    spawn_local(async move {
        link.send_message(Msg::SpinLoaded(api::spin(level, tournament.as_deref()).await));
    });
}

fn fetch_reroll(
    ctx: &Context<Model>,
    kind: RerollKind,
    level: Level,
    tournament: Option<String>,
    year: Option<i32>,
) {
    let link = ctx.link().clone();
    spawn_local(async move {
        let result = api::reroll(kind.as_str(), level, tournament.as_deref(), year).await;
        link.send_message(Msg::RerollResult(kind, result));
    });
}

/// Reveal the next round of the current slam after a short delay (the "aos poucos" effect).
fn schedule_reveal(ctx: &Context<Model>) {
    let link = ctx.link().clone();
    spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(700).await;
        link.send_message(Msg::RevealRound);
    });
}

/// One second of the multiplayer autopick countdown.
fn schedule_tick(ctx: &Context<Model>, seq: u32) {
    let link = ctx.link().clone();
    spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(1000).await;
        link.send_message(Msg::MpTick(seq));
    });
}

/// Reveal one more game of the open knockout match.
fn schedule_game_tick(ctx: &Context<Model>, seq: u32) {
    let link = ctx.link().clone();
    spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(550).await;
        link.send_message(Msg::MpGameTick(seq));
    });
}

/// Pick a random level different from the current one (ATP 250 is never offered).
fn other_level(current: Level) -> Level {
    let options: Vec<Level> = [Level::ATP500, Level::ATP1000, Level::GrandSlam]
        .into_iter()
        .filter(|level| *level != current)
        .collect();
    let index = rand::thread_rng().gen_range(0..options.len());
    options[index]
}

fn next_attribute(picks: &[AttributePickDto]) -> Option<Attribute> {
    Attribute::ALL
        .into_iter()
        .find(|attribute| !picks.iter().any(|pick| pick.attribute == *attribute))
}

fn level_label(level: Level) -> &'static str {
    match level {
        Level::ATP250 => "ATP 250",
        Level::ATP500 => "ATP 500",
        Level::ATP1000 => "ATP 1000",
        Level::GrandSlam => "Grand Slam",
    }
}

fn empty_spin() -> SpinDto {
    SpinDto {
        level: Level::ATP500,
        tournament: "Multiplayer".to_string(),
        year: 2026,
        surface: Surface::Hard,
        players: Vec::new(),
    }
}

fn round_label(index: usize, bracket_size: u8, language: Language) -> &'static str {
    match language {
        Language::Pt => match (bracket_size, index) {
            (16, 0) => "Oitavas",
            (_, 0) => "Quartas",
            (_, 1) if bracket_size == 16 => "Quartas",
            (_, 1) => "Semifinal",
            (_, 2) if bracket_size == 16 => "Semifinal",
            _ => "Final",
        },
        Language::En => match (bracket_size, index) {
            (16, 0) => "Round of 16",
            (_, 0) => "Quarterfinal",
            (_, 1) if bracket_size == 16 => "Quarterfinal",
            (_, 1) => "Semifinal",
            (_, 2) if bracket_size == 16 => "Semifinal",
            _ => "Final",
        },
    }
}

fn main() {
    yew::Renderer::<Model>::new().render();
}
