mod api;
mod sim;

use calendar_slam_shared::{
    Attribute, AttributePickDto, LeaderboardRow, Level, PlayerDto, RunDto, SlamResultDto, SpinDto,
    Surface,
};
use rand::Rng;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;
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

// Which screen is on: the landing page (tutorial + ranking) or the draft/sim game.
#[derive(Default, PartialEq, Clone, Copy)]
enum View {
    #[default]
    Home,
    Game,
}

#[derive(Default)]
struct Model {
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
}

enum Msg {
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
    RunSaved(Result<(), String>),
    LeaderboardLoaded(Result<Vec<LeaderboardRow>, String>),
    ToggleRun(i64),
    RunDetailLoaded(Result<RunDto, String>),
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
        Self::default()
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
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
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        match self.view {
            View::Home => self.view_home(ctx),
            View::Game => self.view_game(ctx),
        }
    }
}

// How-it-works steps shown on the home page (Brazilian Portuguese).
const TUTORIAL: [(&str, &str); 6] = [
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

impl Model {
    fn view_home(&self, ctx: &Context<Self>) -> Html {
        html! {
            <main class="app-shell">
                <section class="hero">
                    <p class="eyebrow">{"CalendarSlam"}</p>
                    <h1>{"Monte o tenista perfeito"}</h1>
                    <p class="hero-lead">
                        {"Gire a roleta de torneios reais do ATP, roube um atributo de cada lenda e leve o seu Frankenstein das quadras para conquistar os quatro Grand Slams."}
                    </p>
                    <button class="primary hero-cta" onclick={ctx.link().callback(|_| Msg::StartGame)}>
                        {"Jogar"}
                    </button>
                </section>

                <section class="panel home-panel">
                    <div class="panel-head"><h2>{"Como funciona"}</h2><span>{"6 passos"}</span></div>
                    <ol class="tutorial">
                        { for TUTORIAL.iter().enumerate().map(|(index, (title, body))| html! {
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
                    <div class="panel-head"><h2>{"Pontos por colocação"}</h2><span>{"por Slam"}</span></div>
                    <div class="points-table">
                        { for POINTS_TABLE.iter().map(|(label, points)| html! {
                            <div class="points-row">
                                <span>{*label}</span>
                                <strong>{points}</strong>
                            </div>
                        }) }
                    </div>
                    <p class="points-note">
                        {"Máximo de 8000 pontos somando os quatro Grand Slams. Empate é decidido pelo overall do tenista."}
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
                        <button class="back-link" onclick={ctx.link().callback(|_| Msg::GoHome)}>{"← Início"}</button>
                        <p class="eyebrow">{"CalendarSlam"}</p>
                        <h1>{"Monte o tenista perfeito"}</h1>
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
                    <h2>{"Atributos"}</h2>
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
                                <span>{attribute.label()}</span>
                                <strong>{pick.map(|pick| pick.rating.to_string()).unwrap_or_else(|| "--".to_string())}</strong>
                                <small>{pick.map(|pick| pick.player.clone()).unwrap_or_else(|| "aguardando pick".to_string())}</small>
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
                    <h2>{"Roleta"}</h2>
                    <span>{ if self.loading { "girando" } else { "pronta" } }</span>
                </div>
                {
                    if let Some(spin) = &self.spin {
                        html! {
                            <>
                                <div class={classes!("roulette-strip", self.loading.then_some("spinning"))}>
                                    <div><span>{"Nivel"}</span><strong>{level_label(spin.level)}</strong></div>
                                    <div><span>{"Torneio"}</span><strong>{&spin.tournament}</strong></div>
                                    <div><span>{"Ano"}</span><strong>{spin.year}</strong></div>
                                    <div><span>{"Piso"}</span><strong>{surface_label(spin.surface)}</strong></div>
                                </div>
                                <div class="reroll-row">
                                    <button disabled={disabled || level_left == 0} onclick={ctx.link().callback(|_| Msg::RerollLevel)}>{format!("Mudar nivel ({level_left})")}</button>
                                    <button disabled={disabled || tournament_left == 0} onclick={ctx.link().callback(|_| Msg::RerollTournament)}>{format!("Mudar torneio ({tournament_left})")}</button>
                                    <button disabled={disabled || year_left == 0} onclick={ctx.link().callback(|_| Msg::RerollYear)}>{format!("Mudar ano ({year_left})")}</button>
                                </div>
                                <div class="player-list">
                                    { for spin.players.iter().cloned().map(|player| {
                                        let used = self.picks.iter().any(|pick| pick.player == player.name);
                                        let selected = self.selected_player.as_ref().map(|p| p.name.as_str()) == Some(player.name.as_str());
                                        let class = classes!("player-row", selected.then_some("selected"), used.then_some("used"));
                                        let rating = self.selected_attribute
                                            .map(|attribute| player.ratings.get(attribute))
                                            .unwrap_or_else(|| player.ratings.get(Attribute::Serve));
                                        let selected_player = player.clone();
                                        html! {
                                            <button class={class} disabled={used} onclick={ctx.link().callback(move |_| Msg::SelectPlayer(selected_player.clone()))}>
                                                <span>{&player.name}</span>
                                                <small>{ if used { "ja usado".to_string() } else { format!("{} | {}", player.best_round, rating) } }</small>
                                            </button>
                                        }
                                    })}
                                </div>
                                { self.view_selected_player_ratings() }
                                <button class="primary" disabled={self.selected_player.is_none() || self.selected_attribute.is_none()} onclick={ctx.link().callback(|_| Msg::ConfirmPick)}>
                                    {"Confirmar atributo"}
                                </button>
                            </>
                        }
                    } else if self.picks.len() == Attribute::ALL.len() {
                        html! { <div class="complete-card">{"Draft completo. Simule os Slams."}</div> }
                    } else {
                        html! { <div class="complete-card">{"Carregando sorteio..."}</div> }
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
                    <span>{format!("{}/4 titulos · {} pts", titles, points)}</span>
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
                                format!("ganhou de {}", round.opponent)
                            } else {
                                format!("perdeu para {}", round.opponent)
                            } }</small>
                        </div>
                    }) }
                </div>
                { if finished {
                    html! {
                        <span class="slam-outcome">
                            { if result.won { "Campeao!".to_string() } else { format!("Eliminado na {}", result.exit_round) } }
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
            return html! { <button class="primary" disabled=true>{"Simulando..."}</button> };
        }
        if all_done {
            return Html::default();
        }
        let name = SLAMS[self.current_slam].1;
        let label = if self.current_slam == 0 {
            format!("Simular {name}")
        } else {
            format!("Proximo: {name}")
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
                <div class="save-run saved">
                    <div class="saved-msg">{format!("Run salva! {} pontos ATP no ranking.", calendar_slam_shared::run_points(&self.sim_results))}</div>
                    <button class="primary" onclick={ctx.link().callback(|_| Msg::GoHome)}>{"Ver ranking"}</button>
                </div>
            };
        }

        html! {
            <div class="save-run">
                <input
                    placeholder="apelido"
                    value={self.nickname.clone()}
                    oninput={ctx.link().callback(|event: InputEvent| {
                        let input: HtmlInputElement = event.target_unchecked_into();
                        Msg::NicknameChanged(input.value())
                    })}
                />
                <button class="primary" disabled={self.nickname.trim().is_empty()} onclick={ctx.link().callback(|_| Msg::SaveRun)}>
                    {"Salvar run"}
                </button>
            </div>
        }
    }

    fn view_selected_player_ratings(&self) -> Html {
        let Some(player) = &self.selected_player else {
            return Html::default();
        };

        html! {
            <div class="ratings-card">
                <div class="ratings-head">
                    <strong>{&player.name}</strong>
                    <span>{format!("melhor rodada: {}", player.best_round)}</span>
                </div>
                <div class="ratings-grid">
                    { for Attribute::ALL.iter().map(|attribute| {
                        let selected = self.selected_attribute == Some(*attribute);
                        html! {
                            <div class={classes!("rating-pill", selected.then_some("selected"))}>
                                <span>{attribute.label()}</span>
                                <strong>{player.ratings.get(*attribute)}</strong>
                            </div>
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
                    <h2>{"Ranking geral"}</h2>
                    <span>{"top 50 · clique p/ ver o time"}</span>
                </div>
                {
                    if self.leaderboard.is_empty() {
                        html! { <div class="empty-rank">{"Nenhuma run salva ainda. Seja o primeiro!"}</div> }
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
            return html! { <div class="run-build loading">{"Carregando time..."}</div> };
        };
        html! {
            <div class="run-build">
                { for Attribute::ALL.iter().map(|attribute| {
                    match run.attributes.iter().find(|pick| pick.attribute == *attribute) {
                        Some(pick) => html! {
                            <div class="build-cell">
                                <span class="build-attr">{attribute.label()}</span>
                                <strong class="build-rating">{pick.rating}</strong>
                                <span class="build-player">{&pick.player}</span>
                                <small class="build-src">{format!("{} {}", pick.source.tournament, pick.source.year)}</small>
                            </div>
                        },
                        None => html! {
                            <div class="build-cell empty">
                                <span class="build-attr">{attribute.label()}</span>
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

fn surface_label(surface: Surface) -> &'static str {
    match surface {
        Surface::Hard => "Hard",
        Surface::Clay => "Saibro",
        Surface::Grass => "Grama",
        Surface::Carpet => "Carpet",
    }
}

fn main() {
    yew::Renderer::<Model>::new().render();
}
