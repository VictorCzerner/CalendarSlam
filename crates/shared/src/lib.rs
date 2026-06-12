use rand::Rng;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Attribute {
    Serve,
    Forehand,
    Backhand,
    Return,
    Movement,
    Mental,
    Net,
    Stamina,
}

impl Attribute {
    pub const ALL: [Attribute; 8] = [
        Attribute::Serve,
        Attribute::Forehand,
        Attribute::Backhand,
        Attribute::Return,
        Attribute::Movement,
        Attribute::Mental,
        Attribute::Net,
        Attribute::Stamina,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Attribute::Serve => "Saque",
            Attribute::Forehand => "Forehand",
            Attribute::Backhand => "Backhand",
            Attribute::Return => "Devolucao",
            Attribute::Movement => "Movimentacao",
            Attribute::Mental => "Mental",
            Attribute::Net => "Voleio/Rede",
            Attribute::Stamina => "Resistencia",
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            Attribute::Serve => "serve",
            Attribute::Forehand => "forehand",
            Attribute::Backhand => "backhand",
            Attribute::Return => "return",
            Attribute::Movement => "movement",
            Attribute::Mental => "mental",
            Attribute::Net => "net",
            Attribute::Stamina => "stamina",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "serve" => Some(Self::Serve),
            "forehand" => Some(Self::Forehand),
            "backhand" => Some(Self::Backhand),
            "return" => Some(Self::Return),
            "movement" => Some(Self::Movement),
            "mental" => Some(Self::Mental),
            "net" => Some(Self::Net),
            "stamina" => Some(Self::Stamina),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Level {
    ATP250,
    ATP500,
    ATP1000,
    GrandSlam,
}

impl Level {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ATP250 => "ATP250",
            Self::ATP500 => "ATP500",
            Self::ATP1000 => "ATP1000",
            Self::GrandSlam => "GrandSlam",
        }
    }
}

impl std::fmt::Display for Level {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Surface {
    Hard,
    Clay,
    Grass,
    Carpet,
}

impl Surface {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hard => "Hard",
            Self::Clay => "Clay",
            Self::Grass => "Grass",
            Self::Carpet => "Carpet",
        }
    }
}

impl std::fmt::Display for Surface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpinDto {
    pub level: Level,
    pub tournament: String,
    pub year: i32,
    pub surface: Surface,
    pub players: Vec<PlayerDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditionDto {
    pub slam: String,
    pub tournament: String,
    pub year: i32,
    pub surface: Surface,
    pub champion: String,
    pub champion_strength: f64,
    /// Real players of this edition (ratings already boosted as Masters 1000 champions), with their
    /// actual `best_round` so the simulation can draw round-appropriate opponents.
    #[serde(default)]
    pub players: Vec<PlayerDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerDto {
    pub name: String,
    pub best_round: String,
    #[serde(default = "PlayerRatings::neutral")]
    pub ratings: PlayerRatings,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PlayerRatings {
    pub serve: u8,
    pub forehand: u8,
    pub backhand: u8,
    pub return_rating: u8,
    pub movement: u8,
    pub mental: u8,
    pub net: u8,
    pub stamina: u8,
}

impl PlayerRatings {
    pub const fn neutral() -> Self {
        Self {
            serve: 55,
            forehand: 55,
            backhand: 55,
            return_rating: 55,
            movement: 55,
            mental: 55,
            net: 55,
            stamina: 55,
        }
    }

    pub const fn get(self, attribute: Attribute) -> u8 {
        match attribute {
            Attribute::Serve => self.serve,
            Attribute::Forehand => self.forehand,
            Attribute::Backhand => self.backhand,
            Attribute::Return => self.return_rating,
            Attribute::Movement => self.movement,
            Attribute::Mental => self.mental,
            Attribute::Net => self.net,
            Attribute::Stamina => self.stamina,
        }
    }

    pub fn buffed(self, level: Level, best_round: &str) -> Self {
        let buff = level_buff(level) + round_buff(best_round);
        Self {
            serve: clamp_rating(i16::from(self.serve) + buff),
            forehand: clamp_rating(i16::from(self.forehand) + buff),
            backhand: clamp_rating(i16::from(self.backhand) + buff),
            return_rating: clamp_rating(i16::from(self.return_rating) + buff),
            movement: clamp_rating(i16::from(self.movement) + buff),
            mental: clamp_rating(i16::from(self.mental) + buff),
            net: clamp_rating(i16::from(self.net) + buff),
            stamina: clamp_rating(i16::from(self.stamina) + buff),
        }
    }

    /// Add a flat amount to every attribute (clamped). Used for a small opponent boost.
    pub fn plus_all(self, amount: i16) -> Self {
        Self {
            serve: clamp_rating(i16::from(self.serve) + amount),
            forehand: clamp_rating(i16::from(self.forehand) + amount),
            backhand: clamp_rating(i16::from(self.backhand) + amount),
            return_rating: clamp_rating(i16::from(self.return_rating) + amount),
            movement: clamp_rating(i16::from(self.movement) + amount),
            mental: clamp_rating(i16::from(self.mental) + amount),
            net: clamp_rating(i16::from(self.net) + amount),
            stamina: clamp_rating(i16::from(self.stamina) + amount),
        }
    }

    pub fn floored(self, attribute: Attribute, floor: u8) -> Self {
        let mut ratings = self;
        match attribute {
            Attribute::Serve => ratings.serve = ratings.serve.max(floor),
            Attribute::Forehand => ratings.forehand = ratings.forehand.max(floor),
            Attribute::Backhand => ratings.backhand = ratings.backhand.max(floor),
            Attribute::Return => ratings.return_rating = ratings.return_rating.max(floor),
            Attribute::Movement => ratings.movement = ratings.movement.max(floor),
            Attribute::Mental => ratings.mental = ratings.mental.max(floor),
            Attribute::Net => ratings.net = ratings.net.max(floor),
            Attribute::Stamina => ratings.stamina = ratings.stamina.max(floor),
        }
        ratings
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributePickDto {
    pub attribute: Attribute,
    pub player: String,
    pub rating: u8,
    pub source: SpinDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundResultDto {
    pub round: String,
    pub opponent: String,
    pub won: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlamResultDto {
    pub slam: String,
    pub tournament: String,
    pub year: i32,
    pub champion: String,
    pub won: bool,
    pub exit_round: String,
    pub rounds_won: u8,
    /// ATP ranking points earned for this slam's finishing position (`slam_points`).
    #[serde(default)]
    pub points: u16,
    #[serde(default)]
    pub lost_to: Option<String>,
    /// Each round played, in order (last one is the loss when `won` is false).
    #[serde(default)]
    pub rounds: Vec<RoundResultDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunDto {
    pub nickname: String,
    pub overall: u8,
    pub slams_won: u8,
    /// Total ATP points across the four slams (the leaderboard's primary ranking metric).
    pub points: u32,
    pub attributes: Vec<AttributePickDto>,
    pub sources: Vec<SlamResultDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedRunDto {
    pub id: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderboardRow {
    pub id: i64,
    pub nickname: String,
    pub overall: u8,
    pub slams_won: u8,
    #[serde(default)]
    pub points: u32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LobbyPlayer {
    pub id: String,
    pub name: String,
    pub is_bot: bool,
    pub connected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MpTeam {
    pub id: String,
    pub name: String,
    pub is_bot: bool,
    pub picks: Vec<AttributePickDto>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetScore {
    pub a: u8,
    pub b: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BracketMatch {
    pub round: u8,
    pub a: String,
    pub b: String,
    pub winner: Option<String>,
    pub surface: Surface,
    /// Set-by-set games score (e.g. [6-4, 3-6, 7-5]). Empty for legacy messages.
    #[serde(default)]
    pub sets: Vec<SetScore>,
    /// Running game-by-game score within each set (`games[i]` ends at `sets[i]`), for live reveal.
    #[serde(default)]
    pub games: Vec<Vec<SetScore>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bracket {
    pub rounds: Vec<Vec<BracketMatch>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MpClientMsg {
    CreateRoom { name: String, bracket_size: u8 },
    JoinRoom { code: String, name: String },
    StartGame,
    MakePick { attribute: Attribute, player: String },
    /// Host-only: advance the knockout reveal to the next match (broadcast to everyone).
    RevealNext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MpServerMsg {
    Joined { your_id: String, code: String },
    RoomState {
        code: String,
        host_id: String,
        bracket_size: u8,
        players: Vec<LobbyPlayer>,
    },
    DraftTurn {
        on_clock: String,
        your_turn: bool,
        spin: SpinDto,
        deadline_ms: u32,
        picks_made: u8,
        total_picks: u8,
    },
    PickMade {
        team_id: String,
        attribute: Attribute,
        player: String,
        rating: u8,
    },
    KnockoutResult {
        bracket: Bracket,
        champion: String,
        #[serde(default)]
        teams: Vec<MpTeam>,
    },
    /// How many knockout matches have been revealed so far (host-driven, synced to all clients).
    RevealAdvance { reveal: u32 },
    Error { message: String },
}

// Edition buff (Fase 6): a 500-champion is the reference (0). Higher levels add, earlier rounds
// subtract, so the achievement in the drawn edition drives the magnitude on top of the low base.
pub const fn level_buff(level: Level) -> i16 {
    match level {
        Level::ATP250 => 0,
        Level::ATP500 => 0,
        Level::ATP1000 => 3,
        Level::GrandSlam => 6,
    }
}

pub const fn round_buff(round: &str) -> i16 {
    match round.as_bytes() {
        b"W" => 0,
        b"F" => -2,
        b"SF" => -4,
        b"QF" => -6,
        b"R16" => -8,
        b"R32" => -10,
        b"R64" => -11,
        b"R128" => -12,
        b"RR" => -6,
        b"BR" => -2,
        _ => -8,
    }
}

/// ATP ranking points for a slam finish, by the round the player exited at.
/// `exit_round` follows the simulation's labels (`R1`,`R2`,`R3`,`R4`,`QF`,`SF`,`F`),
/// or `W` when the slam was won. Mirrors the real Grand Slam point table.
pub const fn slam_points(exit_round: &str, won: bool) -> u16 {
    if won {
        return 2000;
    }
    // Lost at this round.
    match exit_round.as_bytes() {
        b"W" => 2000,
        b"F" => 1200,
        b"SF" => 720,
        b"QF" => 360,
        b"R4" => 180,
        b"R3" => 90,
        b"R2" => 45,
        b"R1" => 10,
        _ => 0,
    }
}

/// Total ATP points across a run's slams (the leaderboard's primary ranking metric).
pub fn run_points(slams: &[SlamResultDto]) -> u32 {
    slams.iter().map(|slam| u32::from(slam.points)).sum()
}

pub const KNOCKOUT_UPSET_CHANCE: f64 = 0.12;

pub fn weight(attribute: Attribute, surface: Surface) -> f64 {
    match surface {
        Surface::Grass => match attribute {
            Attribute::Serve => 0.22,
            Attribute::Net => 0.16,
            Attribute::Forehand => 0.13,
            Attribute::Mental => 0.12,
            Attribute::Movement => 0.11,
            Attribute::Backhand => 0.09,
            Attribute::Stamina => 0.09,
            Attribute::Return => 0.08,
        },
        Surface::Clay => match attribute {
            Attribute::Movement => 0.17,
            Attribute::Stamina => 0.16,
            Attribute::Forehand => 0.15,
            Attribute::Return => 0.16,
            Attribute::Mental => 0.13,
            Attribute::Backhand => 0.11,
            Attribute::Serve => 0.07,
            Attribute::Net => 0.05,
        },
        Surface::Hard | Surface::Carpet => match attribute {
            Attribute::Serve => 0.14,
            Attribute::Forehand => 0.14,
            Attribute::Return => 0.13,
            Attribute::Backhand => 0.12,
            Attribute::Movement => 0.13,
            Attribute::Mental => 0.12,
            Attribute::Stamina => 0.12,
            Attribute::Net => 0.10,
        },
    }
}

pub fn team_strength(team: &[AttributePickDto], surface: Surface) -> f64 {
    team.iter()
        .map(|pick| f64::from(pick.rating) * weight(pick.attribute, surface))
        .sum()
}

pub fn knockout_match(
    a: &[AttributePickDto],
    b: &[AttributePickDto],
    surface: Surface,
    rng: &mut impl Rng,
) -> bool {
    let a_favored = team_strength(a, surface) >= team_strength(b, surface);
    a_favored != rng.gen_bool(KNOCKOUT_UPSET_CHANCE)
}

/// Full outcome of a knockout match: who won, the final set scores, and the running game-by-game
/// score within each set (`games[i]` is the score after each game of set `i`, last = final).
#[derive(Debug, Clone)]
pub struct MatchOutcome {
    pub a_wins: bool,
    pub sets: Vec<SetScore>,
    pub games: Vec<Vec<SetScore>>,
}

/// Per-GAME probability that A wins one game, from the surface-weighted strength gap (logistic).
/// Games are volatile, so this stays near 50/50 — the edge compounds over a set/match.
fn game_win_prob(strength_a: f64, strength_b: f64) -> f64 {
    let diff = strength_a - strength_b;
    let p = 1.0 / (1.0 + (-diff * 0.06).exp());
    p.clamp(0.25, 0.75)
}

/// Play one set game by game following real tennis rules, returning the running score after each
/// game (last element is the final set score). Valid finals only: 6-0..6-4, 7-5, 7-6 (tiebreak).
fn play_set(p_game: f64, rng: &mut impl Rng) -> Vec<SetScore> {
    let mut a = 0u8;
    let mut b = 0u8;
    let mut frames = Vec::new();
    loop {
        if rng.gen_bool(p_game) {
            a += 1;
        } else {
            b += 1;
        }
        frames.push(SetScore { a, b });
        let lead = (i16::from(a) - i16::from(b)).abs();
        // 7-5 / 7-6 (tiebreak), or 6-0..6-4 (win by two at six).
        if a == 7 || b == 7 || (a.max(b) >= 6 && lead >= 2) {
            break;
        }
    }
    frames
}

/// Simulate a best-of-`best_of` match game by game. The strength gap drives each game's odds, so
/// closer matchups go the distance more often and every set/score is tennis-legal.
pub fn simulate_knockout_match(
    a: &[AttributePickDto],
    b: &[AttributePickDto],
    surface: Surface,
    best_of: u8,
    rng: &mut impl Rng,
) -> MatchOutcome {
    let p = game_win_prob(team_strength(a, surface), team_strength(b, surface));
    let need = best_of / 2 + 1;

    let mut won_a = 0u8;
    let mut won_b = 0u8;
    let mut sets = Vec::new();
    let mut games = Vec::new();
    while won_a < need && won_b < need {
        let frames = play_set(p, rng);
        let final_score = *frames.last().expect("a set has at least one game");
        if final_score.a > final_score.b {
            won_a += 1;
        } else {
            won_b += 1;
        }
        sets.push(final_score);
        games.push(frames);
    }
    MatchOutcome {
        a_wins: won_a > won_b,
        sets,
        games,
    }
}

pub const fn clamp_rating(value: i16) -> u8 {
    if value < 0 {
        0
    } else if value > 99 {
        99
    } else {
        value as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffs_match_edition_contract() {
        assert_eq!(level_buff(Level::ATP500), 0);
        assert_eq!(level_buff(Level::ATP1000), 3);
        assert_eq!(level_buff(Level::GrandSlam), 6);
        assert_eq!(round_buff("W"), 0);
        assert_eq!(round_buff("F"), -2);
        assert_eq!(round_buff("SF"), -4);
        assert_eq!(round_buff("QF"), -6);
        assert_eq!(round_buff("R16"), -8);
        assert_eq!(round_buff("R32"), -10);
    }

    #[test]
    fn ratings_are_clamped_after_buff() {
        // serve 96 + GrandSlam(6) + W(0) = 102 -> clamps to 99
        let ratings = PlayerRatings {
            serve: 96,
            forehand: 10,
            backhand: 55,
            return_rating: 55,
            movement: 55,
            mental: 55,
            net: 55,
            stamina: 55,
        }
        .buffed(Level::GrandSlam, "W");

        assert_eq!(ratings.serve, 99);
        assert_eq!(ratings.forehand, 16);
    }

    #[test]
    fn trademark_floor_only_raises_one_attribute() {
        let ratings = PlayerRatings::neutral().floored(Attribute::Serve, 90);
        assert_eq!(ratings.serve, 90);
        assert_eq!(ratings.forehand, 55);
    }

    #[test]
    fn slam_points_match_table() {
        assert_eq!(slam_points("W", true), 2000);
        assert_eq!(slam_points("F", false), 1200);
        assert_eq!(slam_points("SF", false), 720);
        assert_eq!(slam_points("QF", false), 360);
        assert_eq!(slam_points("R4", false), 180);
        assert_eq!(slam_points("R3", false), 90);
        assert_eq!(slam_points("R2", false), 45);
        assert_eq!(slam_points("R1", false), 10);
    }

    #[test]
    fn attribute_keys_roundtrip() {
        for attribute in Attribute::ALL {
            assert_eq!(Attribute::from_key(attribute.key()), Some(attribute));
        }
    }

    #[test]
    fn team_strength_uses_surface_weighting() {
        let hard = SpinDto {
            level: Level::GrandSlam,
            tournament: "Test".to_string(),
            year: 2026,
            surface: Surface::Hard,
            players: Vec::new(),
        };
        let team = vec![
            AttributePickDto {
                attribute: Attribute::Serve,
                player: "A".to_string(),
                rating: 90,
                source: hard.clone(),
            },
            AttributePickDto {
                attribute: Attribute::Return,
                player: "B".to_string(),
                rating: 60,
                source: hard,
            },
        ];

        assert!(team_strength(&team, Surface::Grass) > team_strength(&team, Surface::Clay));
    }

    #[test]
    fn knockout_match_favors_stronger_side_without_upset() {
        struct NoUpset;

        impl rand::RngCore for NoUpset {
            fn next_u32(&mut self) -> u32 {
                u32::MAX
            }

            fn next_u64(&mut self) -> u64 {
                u64::MAX
            }

            fn fill_bytes(&mut self, dest: &mut [u8]) {
                dest.fill(0xff);
            }

            fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
                self.fill_bytes(dest);
                Ok(())
            }
        }

        let source = SpinDto {
            level: Level::GrandSlam,
            tournament: "Test".to_string(),
            year: 2026,
            surface: Surface::Hard,
            players: Vec::new(),
        };
        let strong = Attribute::ALL
            .iter()
            .map(|attribute| AttributePickDto {
                attribute: *attribute,
                player: format!("S{}", attribute.key()),
                rating: 90,
                source: source.clone(),
            })
            .collect::<Vec<_>>();
        let weak = Attribute::ALL
            .iter()
            .map(|attribute| AttributePickDto {
                attribute: *attribute,
                player: format!("W{}", attribute.key()),
                rating: 50,
                source: source.clone(),
            })
            .collect::<Vec<_>>();

        assert!(knockout_match(&strong, &weak, Surface::Hard, &mut NoUpset));
    }

    #[test]
    fn knockout_sets_are_tennis_legal_best_of_five() {
        fn team(rating: u8) -> Vec<AttributePickDto> {
            let source = SpinDto {
                level: Level::GrandSlam,
                tournament: "Test".to_string(),
                year: 2026,
                surface: Surface::Hard,
                players: Vec::new(),
            };
            Attribute::ALL
                .iter()
                .map(|attribute| AttributePickDto {
                    attribute: *attribute,
                    player: format!("{rating}{}", attribute.key()),
                    rating,
                    source: source.clone(),
                })
                .collect()
        }

        fn legal_set(s: &SetScore) -> bool {
            let (hi, lo) = (s.a.max(s.b), s.a.min(s.b));
            matches!((hi, lo), (6, 0..=4) | (7, 5) | (7, 6))
        }

        let a = team(88);
        let b = team(72);
        let mut rng = rand::thread_rng();
        for _ in 0..500 {
            let outcome = simulate_knockout_match(&a, &b, Surface::Hard, 5, &mut rng);
            // Best-of-5: the winner takes exactly 3 sets; 3..=5 sets total.
            assert!((3..=5).contains(&outcome.sets.len()));
            let a_sets = outcome.sets.iter().filter(|s| s.a > s.b).count();
            let b_sets = outcome.sets.len() - a_sets;
            assert_eq!(a_sets.max(b_sets), 3);
            assert_eq!(outcome.a_wins, a_sets > b_sets);
            assert_eq!(outcome.sets.len(), outcome.games.len());
            for (set, frames) in outcome.sets.iter().zip(&outcome.games) {
                assert!(legal_set(set), "illegal set score {:?}", set);
                // Each game advances exactly one point and the last frame is the final score.
                assert_eq!(frames.last(), Some(set));
                let mut prev = SetScore { a: 0, b: 0 };
                for frame in frames {
                    let stepped = frame.a + frame.b == prev.a + prev.b + 1
                        && frame.a >= prev.a
                        && frame.b >= prev.b;
                    assert!(stepped, "non-monotonic frame {:?} after {:?}", frame, prev);
                    prev = *frame;
                }
            }
        }
    }
}
