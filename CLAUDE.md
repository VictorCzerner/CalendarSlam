# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

CalendarSlam is a Rust full-stack browser game: draft a "perfect" tennis player by spinning a roulette of real ATP tournament editions, picking one attribute from one real player per spin (8 attributes total), then simulate the four Grand Slams. Built from Jeff Sackmann's `tennis_atp` match CSVs (CC BY-NC-SA 4.0 — keep visible attribution; not for commercial use). UI copy is Brazilian Portuguese.

## Workspace layout

Cargo workspace with four members:

- `crates/shared` (`calendar-slam-shared`) — DTOs and enums (`Level`, `Surface`, `Attribute`, `PlayerRatings`, `SpinDto`, `EditionDto`, `RunDto`, …) shared by front and back. **Also holds the rating math** (`level_buff`, `round_buff`, `PlayerRatings::buffed/floored/plus_all`) so server and client agree on numbers.
- `server` — Axum + sqlx + Shuttle API. Runs migrations, seeds a fixture, serves JSON under `/api/*`, and serves the embedded WASM front as a fallback.
- `app` — Yew (CSR) + Trunk WASM front. Single `Model` component in `app/src/main.rs`; the slam simulation runs **client-side** in `app/src/sim.rs`.
- `data-prep` (`calendar-slam-data-prep`) — offline CLI ETL that parses the CSVs and populates Postgres. Not part of the running app.

## Commands

One-time setup:

```powershell
rustup target add wasm32-unknown-unknown
cargo install trunk
cargo install cargo-shuttle
```

Build (the front must be built first — the server embeds `app/dist/` at compile time via `rust-embed`):

```powershell
cd app; trunk build --release; cd ..
cargo build
```

Run the API locally (the standalone `shuttle` CLI; Shuttle provisions its own Postgres). Pass `--name CalendarSlam` so it reuses the already-populated shared DB container (`shuttle_CalendarSlam_shared_postgres`) instead of spinning up an empty one:

```powershell
shuttle run --name CalendarSlam
```

Deploy:

```powershell
cd app; trunk build --release; cd ..
shuttle deploy --working-directory server
```

Tests / lint (unit tests live in `shared`, `server::routes`, and `data-prep`):

```powershell
cargo test
cargo test -p calendar-slam-shared buffs_match_edition_contract   # single test
cargo clippy --all-targets
```

ETL against a local Postgres (see `docker-compose.yml` for a throwaway DB):

```powershell
docker compose up -d
cargo run -p calendar-slam-data-prep -- load --raw-dir data/raw --from-year 2000 --to-year 2024 --database-url $env:DATABASE_URL
cargo run -p calendar-slam-data-prep -- validate --database-url $env:DATABASE_URL
cargo run -p calendar-slam-data-prep -- audit500   # print tournaments classified as ATP 500
```

## Gotcha: which Postgres `shuttle run` uses

`shuttle run` provisions and connects to **its own** Postgres (port 15172, container `shuttle_CalendarSlam_shared_postgres`), **not** the `docker-compose.yml` instance. Its connection string is `postgres://postgres:postgres@localhost:15172/CalendarSlam`. Load player profiles into *that* database, or every player's ratings fall back to the neutral default of `55` across the board (see `opt_rating` in `server/src/routes.rs`). The `docker-compose` DB is only for running `data-prep`/`validate` standalone.

Two more rules that follow from this:

- **After changing the rating logic** (`data-prep`'s `signature()` or the Return math), you must re-run `data-prep load` against the shuttle DB; the server reads `player_profiles` live per spin, so no server restart is needed for *data* changes.
- **After changing the contract** (`shared` DTOs) or `server` queries, you must **restart `shuttle run`** — it holds the old compiled binary in memory, and a stale server serving old JSON to a freshly-built front is what produces errors like `missing field` at runtime.

## How the data flows (the part that needs multiple files)

1. **ETL (`data-prep/src/main.rs`)** reads `data/raw/atp_matches_YYYY.csv` and writes three tables:
   - `editions` — one row per (level, tournament, year) with surface + champion. `tourney_level` maps `G→GrandSlam`, `M→ATP1000`, `A→ATP500` *if* the name is in the curated `is_atp_500_name` list else `ATP250`; `D`/`F` (Davis Cup / Tour Finals) are dropped. Editions without a final (`round=F`) are skipped. Tournament names are funneled through `canonical_tournament_name` so spelling/case variants don't split into duplicate rows (this matters — the server's slam lookup is by exact name).
   - `edition_players` — every player who won ≥1 match in that edition, with `best_round` = the furthest round they *reached* (`round_reached_after_win`: a `QF` win means they reached `SF`).
   - `player_profiles` — 8 base ratings per player (`serve, forehand, backhand, return_rating, movement, mental, net, stamina`). Each = `clamp(tier + delta, 60, 95)`. **Tier** is the player's general level: hand-curated in the big `signature()` match block for notable players, else `tier_from_rank` from career-best ATP rank. **Deltas** shape the profile: curated per-attribute deltas, or for non-curated players one `+3` auto-standout on their strongest real signal. A `trademark` is the player's signature attribute with a post-edition floor — `star(...)` gives floor 90, `notable(...)` gives floor 86. Edit `signature()` to change how a real player feels in-game.

### The Devolução (Return) attribute is special

`Attribute::Return` (label "Devolucao", DB column `return_rating` — `return` is a reserved word) replaced the old `Slice`. Unlike the other 7, it is **data-driven for every player** from the real % of return points won, derived from the *opponent's* serve line (`return_points_won / return_points`, accumulated in `observe_player`). The flow in `read_player_profiles`:

- A percentile of that % over players with enough sample maps to a delta via `RETURN_SPREAD`, which **overrides** any curated Return delta. So Return is not hand-tuned per player; it follows the data.
- Then three flat trims are subtracted (all tunable constants): `RETURN_GLOBAL_TRIM` (everyone), `NON_MARCA_RETURN_TRIM` (players whose marca isn't return — keeps Return from rivaling their real signatures), and `return_demotion(name)` (a small hard-coded trim so Djokovic stays alone at the top).
- Return-marca players (Djokovic, Agassi, Murray, Schwartzman, Santoro, Evans, Mannarino) skip `NON_MARCA_RETURN_TRIM` and get the floor-90/86 guarantee in-game regardless of their data base.

2. **Server (`server/src/routes.rs`)** exposes:
   - `GET /api/spin` — random edition at a weighted level (ATP250 never offered; `weighted_level` ≈ 50% 500 / 30% 1000 / 20% Slam), with its players.
   - `GET /api/reroll?kind=level|tournament|year` — swap one facet of the current spin (budget enforced client-side).
   - `GET /api/slam-edition?slam=AO|RG|WIM|USO` — a random edition of that Slam, used as the opponent pool for simulation. Opponents get a flat `OPPONENT_BOOST`.
   - `POST /api/runs` + `GET /api/leaderboard` — persist and rank finished runs (`runs` table, JSONB columns).
   - Player ratings returned to the client are already `buffed()` for the drawn edition's level and the player's round (and floored to the trademark), so the front just reads `player.ratings.get(attribute)`.

3. **Front (`app/src/main.rs`, `app/src/sim.rs`)** drives the draft, then `simulate_slam` plays 7 rounds: your surface-weighted overall vs. each opponent's, with a per-round `UPSET_CHANCE`. Surface weights live in `sim::weight`. The reveal animation steps one round at a time via `schedule_reveal`.

## Rating model invariants

- Ratings are `u8` clamped to `0..=99` everywhere (`clamp_rating` in `shared`).
- `level_buff` + `round_buff` is the single source of truth for edition strength; the `buffs_match_edition_contract` test pins the exact numbers — update the test if you change the curve.
- `Attribute` order is canonical and load-bearing: `Attribute::ALL`, the `attr_index` map in `data-prep`, the `PlayerRatings` field order, and the DB column order must stay aligned. The 4th slot is `Return` (was `Slice`).
- Server `EditionRow`/`PlayerRow` field order mirrors `SELECT *` / explicit column order in the queries; adding a migration column means updating those structs.
- Renaming/replacing an attribute touches every layer at once: the `Attribute` enum + `PlayerRatings` field + `get/buffed/floored` in `shared`, the `PlayerRow` + `SELECT` in `server`, `sim::weight`, the `signature()` deltas + INSERT in `data-prep`, `data/fixture.sql`, and a new migration to rename the column.

## Migrations

Plain SQL in `server/migrations/`, applied at server startup by `db::migrate` (sqlx `migrate!`). `db::seed_fixture_if_empty` runs `data/fixture.sql` only when `editions` is empty (dev convenience until the real dataset is loaded). Add new migrations as `000N_name.sql`; never edit an applied one — e.g. `0006_return_rating.sql` renames the old `slice` column to `return_rating` rather than editing `0004`.
