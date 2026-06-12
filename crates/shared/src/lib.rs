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
}
