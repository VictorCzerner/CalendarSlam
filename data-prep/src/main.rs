use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, Context, Result};
use calendar_slam_shared::{Attribute, Level, Surface};
use clap::{Parser, Subcommand};
use serde::Deserialize;
use sqlx::{postgres::PgPoolOptions, PgPool, Row};

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Load Jeff Sackmann CSV files into Postgres.
    Load {
        #[arg(long, default_value = "data/raw")]
        raw_dir: PathBuf,
        #[arg(long, default_value_t = 2000)]
        from_year: i32,
        #[arg(long, default_value_t = 2024)]
        to_year: i32,
        #[arg(long, env = "DATABASE_URL")]
        database_url: String,
    },
    /// Run Phase 1 readiness checks against the populated database.
    Validate {
        #[arg(long, env = "DATABASE_URL")]
        database_url: String,
    },
    /// Print tournament names classified as ATP 500 from local CSV files.
    Audit500 {
        #[arg(long, default_value = "data/raw")]
        raw_dir: PathBuf,
        #[arg(long, default_value_t = 2000)]
        from_year: i32,
        #[arg(long, default_value_t = 2024)]
        to_year: i32,
    },
}

#[derive(Debug, Deserialize)]
struct MatchRow {
    tourney_name: String,
    surface: String,
    tourney_level: String,
    tourney_date: String,
    winner_rank: Option<i32>,
    loser_rank: Option<i32>,
    winner_ht: Option<f64>,
    loser_ht: Option<f64>,
    winner_name: String,
    loser_name: String,
    score: String,
    round: String,
    minutes: Option<f64>,
    w_ace: Option<f64>,
    w_df: Option<f64>,
    w_svpt: Option<f64>,
    #[serde(rename = "w_1stIn")]
    w_1st_in: Option<f64>,
    #[serde(rename = "w_1stWon")]
    w_1st_won: Option<f64>,
    #[serde(rename = "w_2ndWon")]
    w_2nd_won: Option<f64>,
    #[serde(rename = "w_SvGms")]
    w_sv_gms: Option<f64>,
    #[serde(rename = "w_bpSaved")]
    w_bp_saved: Option<f64>,
    #[serde(rename = "w_bpFaced")]
    w_bp_faced: Option<f64>,
    l_ace: Option<f64>,
    l_df: Option<f64>,
    l_svpt: Option<f64>,
    #[serde(rename = "l_1stIn")]
    l_1st_in: Option<f64>,
    #[serde(rename = "l_1stWon")]
    l_1st_won: Option<f64>,
    #[serde(rename = "l_2ndWon")]
    l_2nd_won: Option<f64>,
    #[serde(rename = "l_SvGms")]
    l_sv_gms: Option<f64>,
    #[serde(rename = "l_bpSaved")]
    l_bp_saved: Option<f64>,
    #[serde(rename = "l_bpFaced")]
    l_bp_faced: Option<f64>,
}

#[derive(Debug, Clone)]
struct EditionAccumulator {
    tournament: String,
    year: i32,
    surface: Surface,
    level: Level,
    champion: Option<String>,
    winners: HashMap<String, String>,
}

#[derive(Debug, Clone, Default)]
struct RawProfile {
    matches: f64,
    wins: f64,
    service_points: f64,
    aces: f64,
    double_faults: f64,
    first_in: f64,
    first_won: f64,
    second_won: f64,
    service_games: f64,
    break_points_saved: f64,
    break_points_faced: f64,
    return_games: f64,
    breaks: f64,
    // Return points faced/won by the player, derived from the opponent's serve line. The real
    // "% de devolução": return_points_won / return_points.
    return_points: f64,
    return_points_won: f64,
    minutes: f64,
    minutes_count: f64,
    long_matches: f64,
    long_wins: f64,
    deciding_sets: f64,
    deciding_set_wins: f64,
    tiebreaks: f64,
    tiebreak_wins: f64,
    upset_chances: f64,
    upset_wins: f64,
    clay_matches: f64,
    clay_wins: f64,
    grass_matches: f64,
    grass_wins: f64,
    height_sum: f64,
    height_count: f64,
    best_rank: Option<i32>,
}

#[derive(Debug, Clone)]
struct PlayerProfile {
    player_name: String,
    serve: u8,
    forehand: u8,
    backhand: u8,
    return_rating: u8,
    movement: u8,
    mental: u8,
    net: u8,
    stamina: u8,
    trademark: Option<Attribute>,
    trademark_floor: Option<u8>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Load {
            raw_dir,
            from_year,
            to_year,
            database_url,
        } => {
            let editions = read_editions(&raw_dir, from_year, to_year)?;
            let profiles = read_player_profiles(&raw_dir, from_year, to_year)?;
            let pool = connect(&database_url).await?;
            load_editions(&pool, editions).await?;
            load_player_profiles(&pool, profiles).await?;
        }
        Command::Validate { database_url } => {
            let pool = connect(&database_url).await?;
            validate(&pool).await?;
        }
        Command::Audit500 {
            raw_dir,
            from_year,
            to_year,
        } => audit_500_names(&raw_dir, from_year, to_year)?,
    }

    Ok(())
}

async fn connect(database_url: &str) -> Result<PgPool> {
    PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
        .context("failed to connect to Postgres")
}

fn read_editions(raw_dir: &Path, from_year: i32, to_year: i32) -> Result<Vec<EditionAccumulator>> {
    let mut editions: BTreeMap<(i32, String), EditionAccumulator> = BTreeMap::new();

    for year in from_year..=to_year {
        let path = raw_dir.join(format!("atp_matches_{year}.csv"));
        if !path.exists() {
            bail!("missing CSV file: {}", path.display());
        }

        let mut reader = csv::Reader::from_path(&path)
            .with_context(|| format!("failed to open {}", path.display()))?;

        for row in reader.deserialize::<MatchRow>() {
            let row = row.with_context(|| format!("invalid row in {}", path.display()))?;
            let tournament = canonical_tournament_name(&row.tourney_name);
            let Some(level) = map_level(&row.tourney_level, &tournament) else {
                continue;
            };
            let surface = map_surface(&row.surface)
                .with_context(|| format!("unsupported surface {:?} in {}", row.surface, path.display()))?;
            let event_year = parse_year(&row.tourney_date)?;
            let key = (event_year, tournament.clone());
            let entry = editions.entry(key).or_insert_with(|| EditionAccumulator {
                tournament: tournament.clone(),
                year: event_year,
                surface,
                level,
                champion: None,
                winners: HashMap::new(),
            });

            if entry.surface != surface {
                bail!(
                    "surface changed within {} {}: {:?} -> {:?}",
                    entry.tournament,
                    entry.year,
                    entry.surface,
                    surface
                );
            }

            if entry.level != level {
                bail!(
                    "level changed within {} {}: {:?} -> {:?}",
                    entry.tournament,
                    entry.year,
                    entry.level,
                    level
                );
            }

            let winner_round = round_reached_after_win(&row.round);
            let current = entry.winners.get(&row.winner_name).cloned();
            if current
                .as_deref()
                .map(|round| round_rank(&winner_round) > round_rank(round))
                .unwrap_or(true)
            {
                entry
                    .winners
                    .insert(row.winner_name.clone(), winner_round);
            }

            if row.round == "F" {
                entry.champion = Some(row.winner_name);
            }
        }
    }

    let mut valid = Vec::with_capacity(editions.len());
    let mut skipped_without_final = 0usize;
    for (_, edition) in editions {
        if edition.champion.is_none() {
            skipped_without_final += 1;
            eprintln!(
                "skipping edition without final/champion: {} {}",
                edition.tournament, edition.year
            );
            continue;
        }
        if edition.winners.is_empty() {
            bail!("edition without match winners: {} {}", edition.tournament, edition.year);
        }
        valid.push(edition);
    }

    if skipped_without_final > 0 {
        eprintln!("skipped {skipped_without_final} editions without final/champion");
    }

    Ok(valid)
}

async fn load_editions(pool: &PgPool, editions: Vec<EditionAccumulator>) -> Result<()> {
    let mut tx = pool.begin().await?;

    for edition in editions {
        let champion = edition
            .champion
            .as_deref()
            .ok_or_else(|| anyhow!("missing champion for {} {}", edition.tournament, edition.year))?;

        let row = sqlx::query(
            r#"
            INSERT INTO editions (level, tournament, year, surface, champion, updated_at)
            VALUES ($1, $2, $3, $4, $5, now())
            ON CONFLICT (level, tournament, year)
            DO UPDATE SET
                surface = EXCLUDED.surface,
                champion = EXCLUDED.champion,
                updated_at = now()
            RETURNING id
            "#,
        )
        .bind(edition.level.as_str())
        .bind(&edition.tournament)
        .bind(edition.year)
        .bind(edition.surface.as_str())
        .bind(champion)
        .fetch_one(&mut *tx)
        .await?;

        let edition_id: i64 = row.try_get("id")?;

        sqlx::query("DELETE FROM edition_players WHERE edition_id = $1")
            .bind(edition_id)
            .execute(&mut *tx)
            .await?;

        for (player_name, best_round) in edition.winners {
            sqlx::query(
                r#"
                INSERT INTO edition_players (edition_id, player_name, best_round)
                VALUES ($1, $2, $3)
                ON CONFLICT (edition_id, player_name)
                DO UPDATE SET best_round = EXCLUDED.best_round
                "#,
            )
            .bind(edition_id)
            .bind(player_name)
            .bind(best_round)
            .execute(&mut *tx)
            .await?;
        }
    }

    tx.commit().await?;
    Ok(())
}

fn read_player_profiles(raw_dir: &Path, from_year: i32, to_year: i32) -> Result<Vec<PlayerProfile>> {
    let mut raw_profiles: HashMap<String, RawProfile> = HashMap::new();

    for year in from_year..=to_year {
        let path = raw_dir.join(format!("atp_matches_{year}.csv"));
        if !path.exists() {
            bail!("missing CSV file: {}", path.display());
        }

        let mut reader = csv::Reader::from_path(&path)
            .with_context(|| format!("failed to open {}", path.display()))?;

        for row in reader.deserialize::<MatchRow>() {
            let row = row.with_context(|| format!("invalid row in {}", path.display()))?;
            let Ok(surface) = map_surface(&row.surface) else {
                eprintln!(
                    "skipping profile row with unsupported surface {:?} in {}",
                    row.surface,
                    path.display()
                );
                continue;
            };
            let score = parse_score_features(&row.score);

            observe_player(&mut raw_profiles, &row, surface, &score, true);
            observe_player(&mut raw_profiles, &row, surface, &score, false);
        }
    }

    let feature_rows = raw_profiles
        .iter()
        .filter(|(_, raw)| raw.matches >= 5.0)
        .map(|(name, raw)| {
            let win_rate = ratio(raw.wins, raw.matches);
            let avg_height = raw.height_sum / raw.height_count.max(1.0);
            let serve_quality = weighted_average(&[
                (ratio(raw.aces, raw.service_points), 0.28),
                (ratio(raw.first_in, raw.service_points), 0.12),
                (ratio(raw.first_won, raw.first_in), 0.22),
                (ratio(raw.second_won, raw.service_points - raw.first_in), 0.18),
                (ratio(raw.break_points_saved, raw.break_points_faced), 0.10),
                (-ratio(raw.double_faults, raw.service_points), 0.10),
                (avg_height / 220.0, 0.10),
            ]);
            // Return ability = real share of return points won (derived from the opponent's serve
            // line). This drives the Return attribute for every player.
            let return_quality = ratio(raw.return_points_won, raw.return_points);
            let stamina_quality = weighted_average(&[
                ((raw.minutes / raw.minutes_count.max(1.0)) / 240.0, 0.34),
                (ratio(raw.long_wins, raw.long_matches), 0.32),
                (ratio(raw.clay_matches, raw.matches), 0.18),
                (ratio(raw.deciding_set_wins, raw.deciding_sets), 0.16),
            ]);
            let mental_quality = weighted_average(&[
                (ratio(raw.break_points_saved, raw.break_points_faced), 0.32),
                (ratio(raw.tiebreak_wins, raw.tiebreaks), 0.24),
                (ratio(raw.deciding_set_wins, raw.deciding_sets), 0.24),
                (ratio(raw.upset_wins, raw.upset_chances), 0.20),
            ]);
            // Movement: clay success + long-match wins (breaks now feed return_quality), with a
            // small penalty for very tall players.
            let movement_quality = weighted_average(&[
                (ratio(raw.clay_wins, raw.clay_matches), 0.42),
                (ratio(raw.long_wins, raw.long_matches), 0.28),
                (win_rate, 0.30),
            ]) - ((avg_height - 188.0).max(0.0) / 1000.0);
            (
                name.clone(),
                ProfileFeatures {
                    serve_quality,
                    return_quality,
                    return_points: raw.return_points,
                    stamina_quality,
                    mental_quality,
                    movement_quality,
                    best_rank: raw.best_rank,
                },
            )
        })
        .collect::<Vec<_>>();

    // Return points needed before we trust a player's return %, so a couple of stat-less matches
    // don't drag a real returner to the bottom of the percentile table.
    const RETURN_MIN_POINTS: f64 = 300.0;
    // Spread of the data-driven Return delta: a top-percentile returner gets about +RETURN_SPREAD/2,
    // a bottom-percentile one about -RETURN_SPREAD/2 (0 at the median).
    const RETURN_SPREAD: f64 = 16.0;
    // Flat trim applied to every player's Return (the whole attribute reads a touch high). Tune here.
    const RETURN_GLOBAL_TRIM: u8 = 1;
    // Extra trim for players whose MARCA is not return, so a strong returner like Nadal doesn't show
    // Devolução level with his real signatures (forehand/movement/stamina).
    const NON_MARCA_RETURN_TRIM: u8 = 2;

    // Population percentiles, used only to pick a single auto-standout for non-curated players.
    let serve_values = values(&feature_rows, |f| f.serve_quality);
    // Return percentiles only over players with a meaningful sample.
    let return_values = {
        let mut v = feature_rows
            .iter()
            .filter(|(_, f)| f.return_points >= RETURN_MIN_POINTS && f.return_quality.is_finite())
            .map(|(_, f)| f.return_quality)
            .collect::<Vec<_>>();
        v.sort_by(|a, b| a.total_cmp(b));
        v
    };
    let stamina_values = values(&feature_rows, |f| f.stamina_quality);
    let mental_values = values(&feature_rows, |f| f.mental_quality);
    let movement_values = values(&feature_rows, |f| f.movement_quality);

    Ok(feature_rows
        .into_iter()
        .map(|(player_name, features)| {
            let curated = signature(&player_name);

            // Tier = the player's general level (the floor for non-signature attributes). Curated for
            // notable players; otherwise derived from their career-best ranking.
            let tier = curated
                .map(|signature| signature.tier)
                .unwrap_or_else(|| tier_from_rank(features.best_rank));

            // Signature deltas (indexed by Attribute order). Curated list, or one auto-standout (+3)
            // on the player's strongest real signal when they are clearly above average.
            let mut deltas = [0i16; 8];
            if let Some(signature) = curated {
                for (attribute, delta) in signature.deltas {
                    deltas[attr_index(*attribute)] += *delta;
                }
            } else {
                // Return is handled separately (data-driven for everyone), so it is not a candidate
                // for the single +3 auto-standout here.
                let signals = [
                    (Attribute::Serve, pct_of(features.serve_quality, &serve_values)),
                    (Attribute::Stamina, pct_of(features.stamina_quality, &stamina_values)),
                    (Attribute::Mental, pct_of(features.mental_quality, &mental_values)),
                    (Attribute::Movement, pct_of(features.movement_quality, &movement_values)),
                ];
                if let Some((attribute, pct)) =
                    signals.iter().max_by(|a, b| a.1.total_cmp(&b.1)).copied()
                {
                    if pct > 0.60 {
                        deltas[attr_index(attribute)] += 3;
                    }
                }
            }

            // Devolução is driven by the real return % for EVERY player (curated or not): map the
            // population percentile to a delta centered on the median. Players without enough data
            // stay neutral. This overrides any hand-written Return delta.
            deltas[attr_index(Attribute::Return)] = if features.return_points >= RETURN_MIN_POINTS {
                let pct = pct_of(features.return_quality, &return_values);
                ((pct - 0.5) * RETURN_SPREAD).round() as i16
            } else {
                0
            };

            let rate = |attribute: Attribute| -> u8 {
                let value = tier + deltas[attr_index(attribute)];
                value.clamp(60, 95) as u8
            };

            // Return-marca players keep their return; everyone else gets it trimmed.
            let non_marca_return_trim = if matches!(
                curated.and_then(|signature| signature.trademark),
                Some((Attribute::Return, _))
            ) {
                0
            } else {
                NON_MARCA_RETURN_TRIM
            };

            PlayerProfile {
                serve: rate(Attribute::Serve),
                forehand: rate(Attribute::Forehand),
                backhand: rate(Attribute::Backhand),
                return_rating: rate(Attribute::Return)
                    .saturating_sub(return_demotion(&player_name))
                    .saturating_sub(RETURN_GLOBAL_TRIM)
                    .saturating_sub(non_marca_return_trim),
                movement: rate(Attribute::Movement),
                mental: rate(Attribute::Mental),
                net: rate(Attribute::Net),
                stamina: rate(Attribute::Stamina),
                trademark: curated.and_then(|signature| signature.trademark.map(|(attribute, _)| attribute)),
                trademark_floor: curated.and_then(|signature| signature.trademark.map(|(_, floor)| floor)),
                player_name,
            }
        })
        .collect())
}

async fn load_player_profiles(pool: &PgPool, profiles: Vec<PlayerProfile>) -> Result<()> {
    let mut tx = pool.begin().await?;

    for profile in profiles {
        sqlx::query(
            r#"
            INSERT INTO player_profiles (
                player_name, serve, forehand, backhand, return_rating, movement, mental, net, stamina,
                trademark, trademark_floor, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, now())
            ON CONFLICT (player_name)
            DO UPDATE SET
                serve = EXCLUDED.serve,
                forehand = EXCLUDED.forehand,
                backhand = EXCLUDED.backhand,
                return_rating = EXCLUDED.return_rating,
                movement = EXCLUDED.movement,
                mental = EXCLUDED.mental,
                net = EXCLUDED.net,
                stamina = EXCLUDED.stamina,
                trademark = EXCLUDED.trademark,
                trademark_floor = EXCLUDED.trademark_floor,
                updated_at = now()
            "#,
        )
        .bind(&profile.player_name)
        .bind(i16::from(profile.serve))
        .bind(i16::from(profile.forehand))
        .bind(i16::from(profile.backhand))
        .bind(i16::from(profile.return_rating))
        .bind(i16::from(profile.movement))
        .bind(i16::from(profile.mental))
        .bind(i16::from(profile.net))
        .bind(i16::from(profile.stamina))
        .bind(profile.trademark.map(|attribute| attribute.key().to_string()))
        .bind(profile.trademark_floor.map(i16::from))
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct ProfileFeatures {
    serve_quality: f64,
    /// Real % of return points won (return_points_won / return_points).
    return_quality: f64,
    /// Total return points with full serve-line data, used to gate noisy small samples.
    return_points: f64,
    stamina_quality: f64,
    mental_quality: f64,
    movement_quality: f64,
    best_rank: Option<i32>,
}

#[derive(Debug, Clone, Copy, Default)]
struct ScoreFeatures {
    sets: u8,
    has_tiebreak: bool,
}

fn observe_player(
    raw_profiles: &mut HashMap<String, RawProfile>,
    row: &MatchRow,
    surface: Surface,
    score: &ScoreFeatures,
    is_winner: bool,
) {
    let player_name = if is_winner {
        &row.winner_name
    } else {
        &row.loser_name
    };
    let profile = raw_profiles.entry(player_name.clone()).or_default();
    let won = if is_winner { 1.0 } else { 0.0 };

    profile.matches += 1.0;
    profile.wins += won;

    match surface {
        Surface::Clay => {
            profile.clay_matches += 1.0;
            profile.clay_wins += won;
        }
        Surface::Grass => {
            profile.grass_matches += 1.0;
            profile.grass_wins += won;
        }
        Surface::Hard | Surface::Carpet => {}
    }

    if let Some(minutes) = row.minutes {
        profile.minutes += minutes;
        profile.minutes_count += 1.0;
        if minutes >= 150.0 {
            profile.long_matches += 1.0;
            profile.long_wins += won;
        }
    }

    if score.sets >= 3 {
        profile.deciding_sets += 1.0;
        profile.deciding_set_wins += won;
    }
    if score.has_tiebreak {
        profile.tiebreaks += 1.0;
        profile.tiebreak_wins += won;
    }

    let (own_rank, opponent_rank) = if is_winner {
        (row.winner_rank, row.loser_rank)
    } else {
        (row.loser_rank, row.winner_rank)
    };
    if let (Some(own), Some(opponent)) = (own_rank, opponent_rank) {
        if own > opponent {
            profile.upset_chances += 1.0;
            profile.upset_wins += won;
        }
    }
    if let Some(own) = own_rank {
        if own >= 1 {
            profile.best_rank = Some(profile.best_rank.map_or(own, |best| best.min(own)));
        }
    }

    let stats = if is_winner {
        ServeStats {
            ace: row.w_ace,
            df: row.w_df,
            svpt: row.w_svpt,
            first_in: row.w_1st_in,
            first_won: row.w_1st_won,
            second_won: row.w_2nd_won,
            sv_gms: row.w_sv_gms,
            bp_saved: row.w_bp_saved,
            bp_faced: row.w_bp_faced,
            height: row.winner_ht,
            opp_sv_gms: row.l_sv_gms,
            opp_bp_saved: row.l_bp_saved,
            opp_bp_faced: row.l_bp_faced,
            opp_svpt: row.l_svpt,
            opp_first_won: row.l_1st_won,
            opp_second_won: row.l_2nd_won,
        }
    } else {
        ServeStats {
            ace: row.l_ace,
            df: row.l_df,
            svpt: row.l_svpt,
            first_in: row.l_1st_in,
            first_won: row.l_1st_won,
            second_won: row.l_2nd_won,
            sv_gms: row.l_sv_gms,
            bp_saved: row.l_bp_saved,
            bp_faced: row.l_bp_faced,
            height: row.loser_ht,
            opp_sv_gms: row.w_sv_gms,
            opp_bp_saved: row.w_bp_saved,
            opp_bp_faced: row.w_bp_faced,
            opp_svpt: row.w_svpt,
            opp_first_won: row.w_1st_won,
            opp_second_won: row.w_2nd_won,
        }
    };

    profile.aces += stats.ace.unwrap_or(0.0);
    profile.double_faults += stats.df.unwrap_or(0.0);
    profile.service_points += stats.svpt.unwrap_or(0.0);
    profile.first_in += stats.first_in.unwrap_or(0.0);
    profile.first_won += stats.first_won.unwrap_or(0.0);
    profile.second_won += stats.second_won.unwrap_or(0.0);
    profile.service_games += stats.sv_gms.unwrap_or(0.0);
    profile.break_points_saved += stats.bp_saved.unwrap_or(0.0);
    profile.break_points_faced += stats.bp_faced.unwrap_or(0.0);
    profile.return_games += stats.opp_sv_gms.unwrap_or(0.0);
    profile.breaks += stats
        .opp_bp_faced
        .zip(stats.opp_bp_saved)
        .map(|(faced, saved)| (faced - saved).max(0.0))
        .unwrap_or(0.0);
    // Return points won = opponent's serve points that the server did NOT win. Only count matches
    // where all three serve-line components are present, so the ratio stays clean.
    if let (Some(svpt), Some(first_won), Some(second_won)) =
        (stats.opp_svpt, stats.opp_first_won, stats.opp_second_won)
    {
        if svpt > 0.0 {
            profile.return_points += svpt;
            profile.return_points_won += (svpt - first_won - second_won).max(0.0);
        }
    }

    if let Some(height) = stats.height {
        profile.height_sum += height;
        profile.height_count += 1.0;
    }
}

#[derive(Debug, Clone, Copy)]
struct ServeStats {
    ace: Option<f64>,
    df: Option<f64>,
    svpt: Option<f64>,
    first_in: Option<f64>,
    first_won: Option<f64>,
    second_won: Option<f64>,
    sv_gms: Option<f64>,
    bp_saved: Option<f64>,
    bp_faced: Option<f64>,
    height: Option<f64>,
    opp_sv_gms: Option<f64>,
    opp_bp_saved: Option<f64>,
    opp_bp_faced: Option<f64>,
    opp_svpt: Option<f64>,
    opp_first_won: Option<f64>,
    opp_second_won: Option<f64>,
}

fn parse_score_features(score: &str) -> ScoreFeatures {
    let mut sets = 0;
    let mut has_tiebreak = false;

    for token in score.split_whitespace() {
        if token.eq_ignore_ascii_case("RET")
            || token.eq_ignore_ascii_case("W/O")
            || token.eq_ignore_ascii_case("DEF")
        {
            continue;
        }
        if token.contains('-') {
            sets += 1;
        }
        if token.contains('(') {
            has_tiebreak = true;
        }
    }

    ScoreFeatures {
        sets,
        has_tiebreak,
    }
}

fn ratio(numerator: f64, denominator: f64) -> f64 {
    if denominator <= 0.0 {
        0.0
    } else {
        numerator / denominator
    }
}

fn weighted_average(values: &[(f64, f64)]) -> f64 {
    let weight_sum = values.iter().map(|(_, weight)| weight).sum::<f64>();
    if weight_sum <= 0.0 {
        return 0.0;
    }
    values
        .iter()
        .map(|(value, weight)| value * weight)
        .sum::<f64>()
        / weight_sum
}

fn values(
    feature_rows: &[(String, ProfileFeatures)],
    f: impl Fn(ProfileFeatures) -> f64,
) -> Vec<f64> {
    let mut values = feature_rows
        .iter()
        .map(|(_, features)| f(*features))
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    values.sort_by(|a, b| a.total_cmp(b));
    values
}

/// Percentile (0..1) of `value` within the sorted population.
fn pct_of(value: f64, sorted_values: &[f64]) -> f64 {
    if sorted_values.is_empty() || !value.is_finite() {
        return 0.5;
    }
    let partition = sorted_values.partition_point(|candidate| *candidate <= value);
    partition as f64 / sorted_values.len() as f64
}

/// Index of an attribute in the canonical 8-slot order (matches `Attribute::ALL`).
fn attr_index(attribute: Attribute) -> usize {
    match attribute {
        Attribute::Serve => 0,
        Attribute::Forehand => 1,
        Attribute::Backhand => 2,
        Attribute::Return => 3,
        Attribute::Movement => 4,
        Attribute::Mental => 5,
        Attribute::Net => 6,
        Attribute::Stamina => 7,
    }
}

/// General level (base floor) of a non-curated player, from their career-best ranking.
/// #1 -> ~90, #3 -> ~86, #10 -> ~81, #20 -> ~77, #50 -> ~72. Clamped to [72, 90].
fn tier_from_rank(best_rank: Option<i32>) -> i16 {
    let rank = best_rank.unwrap_or(300).max(1) as f64;
    (92.0 - 5.0 * rank.ln()).round().clamp(72.0, 90.0) as i16
}

/// Djokovic is the lone reference for a "perfect" return (capped 95). Several other greats hit the
/// same 95 cap from the data; trim them 1-3 points (applied after the clamp) so only Djokovic sits
/// at the top. Magnitude reflects how much of a returner they really were.
fn return_demotion(name: &str) -> u8 {
    match name {
        "Roger Federer" => 3,
        "Yevgeny Kafelnikov" | "Daniil Medvedev" | "Jannik Sinner" | "Carlos Alcaraz" => 2,
        "Rafael Nadal" | "Andy Murray" => 1,
        _ => 0,
    }
}

#[derive(Debug, Clone, Copy)]
struct CuratedSignature {
    tier: i16,
    deltas: &'static [(Attribute, i16)],
    trademark: Option<(Attribute, u8)>,
}

impl CuratedSignature {
    const fn star(tier: i16, trademark: Attribute, deltas: &'static [(Attribute, i16)]) -> Self {
        Self {
            tier,
            deltas,
            trademark: Some((trademark, 90)),
        }
    }

    const fn notable(tier: i16, trademark: Attribute, deltas: &'static [(Attribute, i16)]) -> Self {
        Self {
            tier,
            deltas,
            trademark: Some((trademark, 86)),
        }
    }
}

/// Curated roster of expressive players. Tier is the general base; deltas shape the rest of the
/// profile; trademark is the guaranteed post-edition floor for one signature attribute.
/// Names must match the Jeff Sackmann dataset spelling.
#[rustfmt::skip]
fn signature(name: &str) -> Option<CuratedSignature> {
    use Attribute::{Backhand as BH, Forehand as FH, Mental, Movement as Mov, Net, Return as Ret, Serve, Stamina as Stm};
    let entry = match name {
        // Superstars, peak tier.
        "Roger Federer"          => CuratedSignature::star(90, FH, &[(FH, 5), (Net, 3), (Serve, 2)]),
        "Rafael Nadal"           => CuratedSignature::star(90, FH, &[(FH, 6), (Mov, 4), (Stm, 3), (Serve, -2)]),
        "Novak Djokovic"         => CuratedSignature::star(90, Ret, &[(BH, 5), (Mov, 4), (Mental, 3)]),
        "Carlos Alcaraz"         => CuratedSignature::star(88, Mov, &[(Mov, 5), (FH, 5), (Mental, 3)]),
        "Jannik Sinner"          => CuratedSignature::star(88, BH, &[(BH, 5), (FH, 4), (Mov, 3)]),
        "Daniil Medvedev"        => CuratedSignature::star(88, Mov, &[(Mov, 5), (Mental, 4), (BH, 3), (Net, -2)]),
        "Andy Murray"            => CuratedSignature::star(88, Ret, &[(Mov, 5), (Mental, 4), (BH, 2), (Serve, -1)]),
        "Stan Wawrinka"          => CuratedSignature::star(86, BH, &[(BH, 6), (FH, 4), (Mov, -2)]),
        "Pete Sampras"           => CuratedSignature::star(88, Serve, &[(Serve, 6), (Net, 5), (FH, 2), (BH, -2)]),
        "Andre Agassi"           => CuratedSignature::star(87, Ret, &[(BH, 5), (Mental, 4), (FH, 3), (Serve, -2)]),
        "Lleyton Hewitt"         => CuratedSignature::star(86, Mov, &[(Mov, 6), (Mental, 4), (Stm, 3), (Serve, -3)]),
        "Andy Roddick"           => CuratedSignature::star(86, Serve, &[(Serve, 6), (FH, 3), (BH, -5), (Mov, -3)]),
        "Marat Safin"            => CuratedSignature::star(86, FH, &[(FH, 5), (BH, 4), (Serve, 3), (Mental, -3)]),
        "Juan Carlos Ferrero"    => CuratedSignature::star(86, Mov, &[(Mov, 5), (FH, 3), (Stm, 3), (Serve, -1)]),
        "Gustavo Kuerten"        => CuratedSignature::star(86, BH, &[(BH, 5), (FH, 3), (Mov, 3)]),

        // Stars.
        "David Nalbandian"       => CuratedSignature::star(85, BH, &[(BH, 6), (FH, 4), (Mental, 3), (Stm, -2)]),
        "Nikolay Davydenko"      => CuratedSignature::star(84, BH, &[(BH, 4), (FH, 4), (Mov, 4)]),
        "David Ferrer"           => CuratedSignature::star(85, Stm, &[(Stm, 6), (Mov, 5), (Mental, 3), (Serve, -5)]),
        "Fernando Gonzalez"      => CuratedSignature::star(83, FH, &[(FH, 6), (Serve, 3), (BH, -5)]),
        "Tommy Haas"             => CuratedSignature::star(83, FH, &[(FH, 5), (BH, 4)]),
        "Tommy Robredo"          => CuratedSignature::star(83, Stm, &[(Stm, 6), (FH, 3)]),
        "Carlos Moya"            => CuratedSignature::star(84, FH, &[(FH, 6), (Serve, 3), (BH, -2)]),
        "Gaston Gaudio"          => CuratedSignature::star(83, BH, &[(BH, 6), (Mov, 3), (Serve, -3)]),
        "Nicolas Massu"          => CuratedSignature::star(83, Stm, &[(Stm, 6), (Mental, 3)]),
        "James Blake"            => CuratedSignature::star(83, FH, &[(FH, 6), (Serve, 2), (BH, -2)]),
        "Ivan Ljubicic"          => CuratedSignature::star(83, Serve, &[(Serve, 6), (FH, 3), (Mov, -3)]),
        "Jo-Wilfried Tsonga"     => CuratedSignature::star(84, Serve, &[(Serve, 5), (FH, 4), (Net, 3)]),
        "Robin Soderling"        => CuratedSignature::star(83, FH, &[(FH, 6), (Serve, 4), (Mov, -2)]),
        "Marcos Baghdatis"       => CuratedSignature::star(83, BH, &[(BH, 5), (FH, 3), (Mental, 2)]),
        "Mikhail Youzhny"        => CuratedSignature::star(83, BH, &[(BH, 5), (Mental, 2)]),
        "Fernando Verdasco"      => CuratedSignature::star(83, FH, &[(FH, 6), (Serve, 3), (Mental, -2)]),
        "Juan Martin del Potro"  => CuratedSignature::star(86, FH, &[(FH, 6), (Serve, 4), (Mov, -3)]),
        "Marin Cilic"            => CuratedSignature::star(84, Serve, &[(Serve, 5), (FH, 4), (Mov, -2)]),
        "Kei Nishikori"          => CuratedSignature::star(84, BH, &[(BH, 5), (Mov, 4), (FH, 3), (Serve, -3)]),
        "Tomas Berdych"          => CuratedSignature::star(84, FH, &[(FH, 5), (Serve, 4), (Mov, -2)]),
        "Richard Gasquet"        => CuratedSignature::star(83, BH, &[(BH, 6), (Net, 3), (Mental, -2)]),
        "Gael Monfils"           => CuratedSignature::star(83, Mov, &[(Mov, 6), (Serve, 3), (Mental, -3)]),
        "Grigor Dimitrov"        => CuratedSignature::star(85, BH, &[(BH, 4), (Net, 4), (FH, 3)]),
        "Milos Raonic"           => CuratedSignature::star(83, Serve, &[(Serve, 6), (FH, 3), (Mov, -4)]),
        "John Isner"             => CuratedSignature::star(83, Serve, &[(Serve, 6), (Net, 3), (Mov, -5), (BH, -4)]),
        "Kevin Anderson"         => CuratedSignature::star(83, Serve, &[(Serve, 6), (FH, 3), (Mov, -3)]),
        "Nick Kyrgios"           => CuratedSignature::star(84, Serve, &[(Serve, 6), (FH, 4), (Mental, -3)]),
        "David Goffin"           => CuratedSignature::star(83, Mov, &[(Mov, 5), (BH, 3), (Serve, -3)]),
        "Dominic Thiem"          => CuratedSignature::star(85, BH, &[(BH, 5), (FH, 5), (Stm, 3)]),
        "Alexander Zverev"       => CuratedSignature::star(85, Serve, &[(Serve, 5), (BH, 4), (Mental, -2)]),
        "Stefanos Tsitsipas"     => CuratedSignature::star(84, FH, &[(FH, 5), (Net, 3), (Serve, 3), (BH, -2)]),
        "Andrey Rublev"          => CuratedSignature::star(83, FH, &[(FH, 6), (Serve, 3), (Mental, -3)]),
        "Casper Ruud"            => CuratedSignature::star(83, Mov, &[(Mov, 5), (FH, 5), (Stm, 4), (Net, -2)]),
        "Matteo Berrettini"      => CuratedSignature::star(83, Serve, &[(Serve, 6), (FH, 4), (Mov, -3), (BH, -2)]),
        "Hubert Hurkacz"         => CuratedSignature::star(83, Serve, &[(Serve, 6), (Net, 4)]),
        "Taylor Fritz"           => CuratedSignature::star(83, Serve, &[(Serve, 5), (FH, 4)]),
        "Felix Auger Aliassime"  => CuratedSignature::star(83, Serve, &[(Serve, 5), (FH, 3)]),
        "Holger Rune"            => CuratedSignature::star(83, FH, &[(FH, 5), (Mov, 3), (Mental, -2)]),
        "Frances Tiafoe"         => CuratedSignature::star(83, Mov, &[(Mov, 5), (FH, 3), (Mental, 2)]),
        "Cameron Norrie"         => CuratedSignature::star(83, Stm, &[(Stm, 5), (Mov, 4), (Serve, -2)]),

        // Masters 1000+ champions promoted to star (floor 90).
        "Goran Ivanisevic"       => CuratedSignature::star(85, Serve, &[(Serve, 6), (Net, 4), (Mov, -2)]),
        "Alex Corretja"          => CuratedSignature::star(84, Mov, &[(Mov, 5), (Stm, 4), (Serve, -2)]),
        "Magnus Norman"          => CuratedSignature::star(84, FH, &[(FH, 5), (Mov, 3), (Stm, 3)]),
        "Thomas Enqvist"         => CuratedSignature::star(83, FH, &[(FH, 6), (Serve, 4), (Mov, -2)]),
        "Guillermo Canas"        => CuratedSignature::star(82, Mov, &[(Mov, 5), (Stm, 4)]),
        "Wayne Ferreira"         => CuratedSignature::star(82, FH, &[(FH, 4), (Serve, 3)]),
        "Cedric Pioline"         => CuratedSignature::star(82, Serve, &[(Serve, 4), (BH, 4)]),
        // Popyrin/Portas: one-off Masters winners (Montreal 2024 / Hamburg 2001), otherwise ~#19
        // journeymen -> not star-floor material.
        "Alexei Popyrin"         => CuratedSignature::notable(79, Serve, &[(Serve, 5), (FH, 4)]),
        "Felix Mantilla"         => CuratedSignature::star(80, FH, &[(FH, 5), (Mov, 3), (Serve, -3)]),
        "Andrei Pavel"           => CuratedSignature::star(80, FH, &[(FH, 4), (Serve, 2)]),
        "Albert Portas"          => CuratedSignature::notable(78, Mov, &[(Mov, 5), (Stm, 3), (Serve, -2)]),

        // Very good / memorable.
        "Guillermo Coria"        => CuratedSignature::star(82, Mov, &[(Mov, 6), (Stm, 4), (Serve, -2), (Mental, -3)]),
        "Fabrice Santoro"        => CuratedSignature::notable(80, Ret, &[(Net, 4), (Serve, -3)]),
        "Tim Henman"             => CuratedSignature::star(81, Net, &[(Net, 6), (Serve, 3), (Mov, 2)]),
        "Max Mirnyi"             => CuratedSignature::notable(78, Serve, &[(Serve, 6), (Net, 5), (Mov, -4)]),
        "Sebastien Grosjean"     => CuratedSignature::star(80, Mov, &[(Mov, 5), (FH, 3)]),
        "Arnaud Clement"         => CuratedSignature::star(82, Mov, &[(Mov, 5), (Stm, 3), (Serve, -3)]),
        "Jarkko Nieminen"        => CuratedSignature::notable(79, Mov, &[(Mov, 5), (BH, 3)]),
        "Radek Stepanek"         => CuratedSignature::star(83, Net, &[(Net, 6), (Ret, 3), (Serve, 2)]),
        "Mardy Fish"             => CuratedSignature::star(83, Serve, &[(Serve, 5), (Net, 3)]),
        "Juan Ignacio Chela"     => CuratedSignature::notable(79, BH, &[(BH, 4), (Stm, 3)]),
        "Rainer Schuettler"      => CuratedSignature::star(84, Mov, &[(Mov, 5), (Stm, 3)]),
        "Dominik Hrbaty"         => CuratedSignature::notable(79, FH, &[(FH, 5), (Stm, 2)]),
        "Nicolas Kiefer"         => CuratedSignature::star(84, BH, &[(BH, 4), (FH, 2)]),
        "Xavier Malisse"         => CuratedSignature::notable(79, BH, &[(BH, 4), (FH, 3)]),
        "Paradorn Srichaphan"    => CuratedSignature::star(82, FH, &[(FH, 5), (Serve, 2)]),
        "Younes El Aynaoui"      => CuratedSignature::notable(79, FH, &[(FH, 5), (Serve, 3)]),
        "Albert Costa"           => CuratedSignature::star(80, Mov, &[(Mov, 5), (Stm, 3)]),
        "Thomas Johansson"       => CuratedSignature::star(80, FH, &[(FH, 4), (Serve, 3)]),
        "Jurgen Melzer"          => CuratedSignature::star(83, Net, &[(Net, 4), (BH, 3)]),
        "Olivier Rochus"         => CuratedSignature::notable(78, Mov, &[(Mov, 6), (Serve, -5)]),
        "Igor Andreev"           => CuratedSignature::notable(79, FH, &[(FH, 5), (Mov, 2)]),
        "Paul Henri Mathieu"     => CuratedSignature::notable(79, FH, &[(FH, 4), (BH, 3)]),
        "Fabio Fognini"          => CuratedSignature::star(80, BH, &[(BH, 5), (FH, 3), (Mental, -3)]),
        "Gilles Simon"           => CuratedSignature::star(84, Mov, &[(Mov, 5), (Mental, 4), (Serve, -4), (Net, -2)]),
        "Philipp Kohlschreiber"  => CuratedSignature::notable(80, BH, &[(BH, 5), (Net, 2)]),
        "Feliciano Lopez"        => CuratedSignature::notable(80, Serve, &[(Serve, 5), (Net, 5), (BH, -2)]),
        "Ivo Karlovic"           => CuratedSignature::notable(80, Serve, &[(Serve, 6), (Net, 4), (Mov, -6), (BH, -4)]),
        "Sam Querrey"            => CuratedSignature::notable(80, Serve, &[(Serve, 6), (FH, 2), (Mov, -3)]),
        "Ernests Gulbis"         => CuratedSignature::star(82, FH, &[(FH, 5), (Serve, 3), (Mental, -4)]),
        "Jeremy Chardy"          => CuratedSignature::notable(79, FH, &[(FH, 5), (Serve, 3)]),
        "Pablo Cuevas"           => CuratedSignature::notable(79, BH, &[(BH, 4), (Mov, 3)]),
        "Benoit Paire"           => CuratedSignature::notable(79, BH, &[(BH, 5), (Mental, -5)]),
        "Jack Sock"              => CuratedSignature::star(80, FH, &[(FH, 6), (Serve, 2)]),
        "Bernard Tomic"          => CuratedSignature::notable(79, Serve, &[(Serve, 4), (Net, 3), (Mental, -4)]),
        "Lucas Pouille"          => CuratedSignature::star(82, FH, &[(FH, 5), (Serve, 2)]),
        "Gilles Muller"          => CuratedSignature::notable(79, Serve, &[(Serve, 6), (Net, 4)]),
        "Adrian Mannarino"       => CuratedSignature::notable(79, Ret, &[(Mov, 2)]),
        "Viktor Troicki"         => CuratedSignature::notable(79, Serve, &[(Serve, 4), (BH, 2)]),
        "Janko Tipsarevic"       => CuratedSignature::star(83, BH, &[(BH, 5), (Mental, 2)]),
        "Alexandr Dolgopolov"    => CuratedSignature::notable(79, Mov, &[(Mov, 5), (Mental, -3)]),
        "Robin Haase"            => CuratedSignature::notable(79, FH, &[(FH, 4), (Serve, 2)]),
        "Steve Johnson"          => CuratedSignature::notable(79, Serve, &[(Serve, 5), (Net, 2)]),
        "Nicolas Mahut"          => CuratedSignature::notable(79, Net, &[(Net, 6), (Serve, 4)]),
        "Juan Monaco"            => CuratedSignature::star(82, Mov, &[(Mov, 5), (Stm, 3)]),
        "Roberto Bautista Agut"  => CuratedSignature::star(82, Mental, &[(Mental, 5), (Mov, 3), (Stm, 3)]),
        "Florian Mayer"          => CuratedSignature::notable(79, Net, &[(Net, 5), (BH, 2)]),
        "Julien Benneteau"       => CuratedSignature::notable(79, Net, &[(Net, 5), (BH, 2)]),
        "Karen Khachanov"        => CuratedSignature::star(81, FH, &[(FH, 4), (Serve, 4)]),
        "Diego Schwartzman"      => CuratedSignature::star(83, Ret, &[(Mov, 6), (Stm, 5), (BH, 3), (Serve, -6), (Net, -3)]),
        "Pablo Carreno Busta"    => CuratedSignature::star(80, Mov, &[(Mov, 5), (BH, 3)]),
        "Denis Shapovalov"       => CuratedSignature::star(82, FH, &[(FH, 5), (Serve, 3), (Mental, -3)]),
        "Lorenzo Musetti"        => CuratedSignature::notable(80, BH, &[(BH, 5), (Net, 3)]),
        "Sebastian Korda"        => CuratedSignature::notable(80, FH, &[(FH, 5), (BH, 3)]),
        "Tommy Paul"             => CuratedSignature::notable(80, Mov, &[(Mov, 5), (FH, 3)]),
        "Ben Shelton"            => CuratedSignature::notable(80, Serve, &[(Serve, 6), (FH, 3), (BH, -2)]),
        "Alex De Minaur"         => CuratedSignature::star(84, Mov, &[(Mov, 6), (Stm, 4), (Serve, -3)]),
        "Jan Lennard Struff"     => CuratedSignature::notable(79, Serve, &[(Serve, 5), (FH, 3)]),
        "Reilly Opelka"          => CuratedSignature::notable(80, Serve, &[(Serve, 6), (Mov, -5), (BH, -3)]),
        "Alexander Bublik"       => CuratedSignature::notable(80, Serve, &[(Serve, 5), (Net, 2)]),
        "Sebastian Baez"         => CuratedSignature::notable(80, Mov, &[(Mov, 6), (Stm, 4), (Serve, -3), (Net, -3)]),
        "Francisco Cerundolo"    => CuratedSignature::notable(80, FH, &[(FH, 5), (Mov, 2)]),
        "Alejandro Davidovich Fokina" => CuratedSignature::notable(80, Mov, &[(Mov, 5), (BH, 3), (Mental, -2)]),
        "Lorenzo Sonego"         => CuratedSignature::notable(79, Serve, &[(Serve, 5), (FH, 3)]),
        "Jiri Lehecka"           => CuratedSignature::notable(79, FH, &[(FH, 5), (Serve, 2)]),
        "Daniel Evans"           => CuratedSignature::notable(79, Ret, &[(Net, 3), (Serve, -2)]),
        "Aslan Karatsev"         => CuratedSignature::notable(79, FH, &[(FH, 5), (BH, 3)]),
        "Borna Coric"            => CuratedSignature::star(80, Mov, &[(Mov, 5), (BH, 3)]),
        "Botic Van De Zandschulp" => CuratedSignature::notable(79, FH, &[(FH, 5), (Serve, 2)]),

        // Former #1s and Slam champions that were missing a marca.
        "Yevgeny Kafelnikov"     => CuratedSignature::star(86, BH, &[(BH, 5), (FH, 2), (Stm, 2)]),
        "Patrick Rafter"         => CuratedSignature::star(85, Net, &[(Net, 6), (Serve, 4), (Mov, 2)]),
        "Marcelo Rios"           => CuratedSignature::star(85, Mov, &[(Mov, 5), (BH, 4), (Net, 2)]),
        "Richard Krajicek"       => CuratedSignature::star(84, Serve, &[(Serve, 6), (Net, 4), (Mov, -2)]),

        // Top-10 / Slam finalists that were missing a marca.
        "Todd Martin"            => CuratedSignature::star(83, Serve, &[(Serve, 5), (BH, 3), (Mov, -2)]),
        "Jiri Novak"             => CuratedSignature::star(82, BH, &[(BH, 4), (Mov, 3), (Net, 2)]),
        "Mark Philippoussis"     => CuratedSignature::star(82, Serve, &[(Serve, 6), (FH, 3), (Mov, -3)]),
        "Nicolas Almagro"        => CuratedSignature::star(82, FH, &[(FH, 5), (BH, 4), (Serve, 2)]),
        "Mario Ancic"            => CuratedSignature::star(82, Serve, &[(Serve, 5), (Net, 4), (FH, 2)]),
        "Nicolas Lapentti"       => CuratedSignature::star(81, FH, &[(FH, 4), (Mov, 3), (Stm, 2)]),
        "Joachim Johansson"      => CuratedSignature::star(81, Serve, &[(Serve, 6), (FH, 3), (Mov, -4)]),
        "Karol Kucera"           => CuratedSignature::notable(81, BH, &[(BH, 4), (Mov, 3), (Mental, 2)]),
        "Ugo Humbert"            => CuratedSignature::notable(80, Serve, &[(Serve, 4), (FH, 3)]),
        "Cristian Garin"         => CuratedSignature::notable(79, Mov, &[(Mov, 5), (Stm, 3), (Serve, -2)]),

        _ => return None,
    };
    Some(entry)
}

async fn validate(pool: &PgPool) -> Result<()> {
    let washington = sqlx::query(
        r#"
        SELECT id, level, surface, champion
        FROM editions
        WHERE tournament = 'Washington' AND year = 2022
        "#,
    )
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow!("Washington 2022 not found"))?;

    let edition_id: i64 = washington.try_get("id")?;
    let level: String = washington.try_get("level")?;
    let surface: String = washington.try_get("surface")?;
    let champion: String = washington.try_get("champion")?;

    if level != "ATP500" || surface != "Hard" || champion != "Nick Kyrgios" {
        bail!(
            "Washington 2022 mismatch: level={level}, surface={surface}, champion={champion}"
        );
    }

    let kyrgios_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM edition_players WHERE edition_id = $1 AND player_name = 'Nick Kyrgios'",
    )
    .bind(edition_id)
    .fetch_one(pool)
    .await?;
    if kyrgios_count != 1 {
        bail!("Nick Kyrgios missing from Washington 2022 edition_players");
    }

    let missing_champions: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM editions WHERE champion IS NULL OR champion = ''")
            .fetch_one(pool)
            .await?;
    if missing_champions != 0 {
        bail!("{missing_champions} editions have null/blank champion");
    }

    let rows = sqlx::query(
        r#"
        SELECT year, level, COUNT(*) AS count
        FROM editions
        GROUP BY year, level
        ORDER BY year, level
        "#,
    )
    .fetch_all(pool)
    .await?;

    println!("Phase 1 validation passed.");
    println!("Tournament counts by year/level:");
    for row in rows {
        let year: i32 = row.try_get("year")?;
        let level: String = row.try_get("level")?;
        let count: i64 = row.try_get("count")?;
        println!("{year} {level}: {count}");
    }

    let players = sqlx::query_scalar::<_, String>(
        r#"
        SELECT ep.player_name
        FROM edition_players ep
        JOIN editions e ON e.id = ep.edition_id
        WHERE e.level = 'ATP500' AND e.tournament = 'Washington' AND e.year = 2022
        ORDER BY ep.player_name
        "#,
    )
    .fetch_all(pool)
    .await?;
    println!("Washington 2022 players with >=1 win: {}", players.join(", "));

    Ok(())
}

fn audit_500_names(raw_dir: &Path, from_year: i32, to_year: i32) -> Result<()> {
    let mut names = BTreeSet::new();
    for year in from_year..=to_year {
        let path = raw_dir.join(format!("atp_matches_{year}.csv"));
        if !path.exists() {
            bail!("missing CSV file: {}", path.display());
        }
        let mut reader = csv::Reader::from_path(&path)?;
        for row in reader.deserialize::<MatchRow>() {
            let row = row?;
            if row.tourney_level == "A" && is_atp_500_name(&row.tourney_name) {
                names.insert(row.tourney_name);
            }
        }
    }

    for name in names {
        println!("{name}");
    }
    Ok(())
}

fn parse_year(tourney_date: &str) -> Result<i32> {
    let year = tourney_date
        .get(0..4)
        .ok_or_else(|| anyhow!("invalid tourney_date: {tourney_date}"))?
        .parse::<i32>()?;
    Ok(year)
}

fn map_level(raw: &str, tournament: &str) -> Option<Level> {
    match raw {
        "G" => Some(Level::GrandSlam),
        "M" => Some(Level::ATP1000),
        "A" if is_atp_500_name(tournament) => Some(Level::ATP500),
        "A" => Some(Level::ATP250),
        "D" | "F" => None,
        _ => None,
    }
}

fn map_surface(raw: &str) -> Result<Surface> {
    match raw {
        "Hard" => Ok(Surface::Hard),
        "Clay" => Ok(Surface::Clay),
        "Grass" => Ok(Surface::Grass),
        "Carpet" => Ok(Surface::Carpet),
        _ => bail!("unsupported surface: {raw}"),
    }
}

fn is_atp_500_name(name: &str) -> bool {
    matches!(
        normalize_name(name).as_str(),
        "acapulco"
            | "barcelona"
            | "basel"
            | "beijing"
            | "dubai"
            | "halle"
            | "hamburg"
            | "london queens club"
            | "london queen s club"
            | "queen s club"
            | "memphis"
            | "rotterdam"
            | "rio de janeiro"
            | "stuttgart"
            | "tokyo"
            | "valencia"
            | "vienna"
            | "washington"
    )
}

/// Collapse whitespace and fold known spelling/case variants of a tournament name
/// into a single canonical form, so each real event maps to exactly one `editions` row.
/// Without this the same tournament leaks into the DB twice (e.g. "US Open"/"Us Open"),
/// which breaks the hardcoded slam lookup in the server and splits roulette buckets.
fn canonical_tournament_name(name: &str) -> String {
    let trimmed = name.split_whitespace().collect::<Vec<_>>().join(" ");
    match normalize_name(&trimmed).as_str() {
        "us open" => "US Open".to_string(),
        "rio de janeiro" => "Rio de Janeiro".to_string(),
        "st petersburg" => "St. Petersburg".to_string(),
        _ => trimmed,
    }
}

fn normalize_name(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn round_rank(round: &str) -> u8 {
    match round {
        "RR" => 0,
        "R128" => 1,
        "R64" => 2,
        "R32" => 3,
        "R16" => 4,
        "QF" => 5,
        "SF" => 6,
        "F" => 7,
        "W" => 8,
        "BR" => 9,
        _ => 0,
    }
}

fn round_reached_after_win(round_won: &str) -> String {
    match round_won {
        "R128" => "R64",
        "R64" => "R32",
        "R32" => "R16",
        "R16" => "QF",
        "QF" => "SF",
        "SF" => "F",
        "F" => "W",
        other => other,
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_washington_as_500() {
        assert_eq!(map_level("A", "Washington"), Some(Level::ATP500));
    }

    #[test]
    fn excludes_davis_cup_and_finals() {
        assert_eq!(map_level("D", "Davis Cup"), None);
        assert_eq!(map_level("F", "Tour Finals"), None);
    }

    #[test]
    fn normalizes_queens_name() {
        assert!(is_atp_500_name("London / Queen's Club"));
    }

    #[test]
    fn canonicalizes_tournament_name_variants() {
        assert_eq!(canonical_tournament_name("Us Open"), "US Open");
        assert_eq!(canonical_tournament_name("US Open"), "US Open");
        assert_eq!(canonical_tournament_name("Rio De Janeiro"), "Rio de Janeiro");
        assert_eq!(canonical_tournament_name("St Petersburg"), "St. Petersburg");
        assert_eq!(canonical_tournament_name("St. Petersburg"), "St. Petersburg");
        assert_eq!(canonical_tournament_name("Belgrade "), "Belgrade");
        assert_eq!(canonical_tournament_name("Washington"), "Washington");
    }

    #[test]
    fn best_round_is_round_reached_after_win() {
        assert_eq!(round_reached_after_win("R128"), "R64");
        assert_eq!(round_reached_after_win("R64"), "R32");
        assert_eq!(round_reached_after_win("R32"), "R16");
        assert_eq!(round_reached_after_win("R16"), "QF");
        assert_eq!(round_reached_after_win("QF"), "SF");
        assert_eq!(round_reached_after_win("SF"), "F");
        assert_eq!(round_reached_after_win("F"), "W");
    }
}
