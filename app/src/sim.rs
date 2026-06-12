use rand::Rng;
use calendar_slam_shared::{
    slam_points, team_strength, weight, Attribute, AttributePickDto, EditionDto, PlayerDto,
    PlayerRatings, RoundResultDto, SlamResultDto, Surface,
};

const ROUNDS: [&str; 7] = ["R1", "R2", "R3", "R4", "QF", "SF", "F"];
// Chance the lower-overall side wins anyway (upset), rising deep into the draw.
const UPSET_CHANCE: [f64; 7] = [0.005, 0.01, 0.02, 0.035, 0.05, 0.075, 0.10];

pub fn overall(attributes: &[AttributePickDto]) -> u8 {
    if attributes.is_empty() {
        return 0;
    }

    let total: u16 = attributes.iter().map(|pick| u16::from(pick.rating)).sum();
    (total / attributes.len() as u16) as u8
}

pub fn strength_for_surface(attributes: &[AttributePickDto], surface: Surface) -> f64 {
    team_strength(attributes, surface)
}

/// Surface-weighted overall (0..99) of a full 8-attribute profile (an opponent).
fn ratings_overall(ratings: &PlayerRatings, surface: Surface) -> f64 {
    Attribute::ALL
        .iter()
        .map(|attribute| f64::from(ratings.get(*attribute)) * weight(*attribute, surface))
        .sum()
}

/// Furthest round a player REACHED in the edition (`best_round` in the data is the round reached:
/// champion = "W", finalist = "F", etc.). Champion and finalist both played the final (7).
fn depth_played(best_round: &str) -> usize {
    match best_round {
        "W" | "F" => 7,
        "SF" => 6,
        "QF" => 5,
        "R16" => 4,
        "R32" => 3,
        "R64" => 2,
        _ => 1,
    }
}

/// Pick an opponent who reached at least `want_depth`, never one already faced this slam.
/// If everyone deep enough was already faced, relax the depth (e.g. the champion was used
/// earlier -> a semifinalist plays the final).
fn pick_opponent<'a>(
    edition: &'a EditionDto,
    want_depth: usize,
    used: &[String],
    rng: &mut impl Rng,
) -> Option<&'a PlayerDto> {
    let mut threshold = want_depth;
    loop {
        let pool: Vec<&PlayerDto> = edition
            .players
            .iter()
            .filter(|player| {
                depth_played(&player.best_round) >= threshold
                    && !used.iter().any(|name| name == &player.name)
            })
            .collect();
        if !pool.is_empty() {
            return Some(pool[rng.gen_range(0..pool.len())]);
        }
        if threshold == 0 {
            return None;
        }
        threshold -= 1;
    }
}

pub fn simulate_slam(attributes: &[AttributePickDto], edition: &EditionDto) -> SlamResultDto {
    let your_overall = strength_for_surface(attributes, edition.surface);
    let mut rng = rand::thread_rng();
    let mut rounds_won = 0u8;
    let mut rounds: Vec<RoundResultDto> = Vec::new();
    let mut used: Vec<String> = Vec::new();

    for (index, round) in ROUNDS.iter().enumerate() {
        let (opponent_name, you_win) = match pick_opponent(edition, index + 1, &used, &mut rng) {
            Some(opponent) => {
                // Higher overall wins, unless an upset (rising per round) flips the result.
                let you_favored =
                    your_overall >= ratings_overall(&opponent.ratings, edition.surface);
                (
                    opponent.name.clone(),
                    you_favored != rng.gen_bool(UPSET_CHANCE[index]),
                )
            }
            None => ("(bye)".to_string(), true),
        };
        used.push(opponent_name.clone());
        rounds.push(RoundResultDto {
            round: (*round).to_string(),
            opponent: opponent_name.clone(),
            won: you_win,
        });

        if you_win {
            rounds_won += 1;
        } else {
            return SlamResultDto {
                slam: edition.slam.clone(),
                tournament: edition.tournament.clone(),
                year: edition.year,
                champion: edition.champion.clone(),
                won: false,
                exit_round: (*round).to_string(),
                rounds_won,
                points: slam_points(round, false),
                lost_to: Some(opponent_name),
                rounds,
            };
        }
    }

    SlamResultDto {
        slam: edition.slam.clone(),
        tournament: edition.tournament.clone(),
        year: edition.year,
        champion: edition.champion.clone(),
        won: true,
        exit_round: "W".to_string(),
        rounds_won: 7,
        points: slam_points("W", true),
        lost_to: None,
        rounds,
    }
}
