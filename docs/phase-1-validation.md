# Phase 1 validation report

## Source

Dataset: Jeff Sackmann `tennis_atp`, files `atp_matches_YYYY.csv`.

Confirmed in `atp_matches_2022.csv`:

- Required columns are present: `tourney_name`, `surface`, `tourney_level`, `round`, `winner_name`, `loser_name`, `tourney_date`.
- `tourney_level` uses `G` for Grand Slam, `M` for Masters 1000, `A` for tour-level events, with `D` and `F` excluded from the game load.
- Washington 2022 appears as `tourney_name=Washington`, `surface=Hard`, `tourney_level=A`, `round=F`, `winner_name=Nick Kyrgios`.

License note: Jeff Sackmann's tennis data is distributed under CC BY-NC-SA 4.0. The app should keep visible attribution in `README` and an About/Credits screen, and should not be used commercially without resolving license terms.

## ATP 500 split

The raw CSV does not split `A` into ATP 250 and ATP 500. The ETL uses a curated name list:

- Acapulco
- Barcelona
- Basel
- Beijing
- Dubai
- Halle
- Hamburg
- London / Queen's Club
- Memphis
- Rotterdam
- Rio de Janeiro
- Stuttgart
- Tokyo
- Valencia
- Vienna
- Washington

Run `cargo run -p calendar-slam-data-prep -- audit500` after downloading all CSV files to print the names matched locally.

## Readiness checks

`cargo run -p calendar-slam-data-prep -- validate` checks:

- Washington 2022 exists as `ATP500`, `Hard`, champion `Nick Kyrgios`.
- Nick Kyrgios exists in `edition_players` for that edition.
- No edition has a blank champion.
- Counts by `year, level` are printed for plausibility review.
- The manual roulette query for `ATP500/Washington/2022` returns the player list.

## Local validation run

Executed on 2026-06-10 against local Postgres `calendarslam`, using CSVs `2000..=2024`.

- Loaded editions: 1605.
- Loaded edition-player rows: 35553.
- Blank/null champions: 0.
- Washington 2022: `level=ATP500`, `surface=Hard`, `champion=Nick Kyrgios`.
- Washington 2022 player list includes `Nick Kyrgios`.
- Skipped editions without `round=F`: `Laver Cup` 2017, 2018, 2019, 2021, 2022, 2024.

ATP 500 names matched in the downloaded CSV files:

- Acapulco
- Barcelona
- Basel
- Beijing
- Dubai
- Halle
- Hamburg
- Memphis
- Queen's Club
- Rio De Janeiro
- Rio de Janeiro
- Rotterdam
- Stuttgart
- Tokyo
- Valencia
- Vienna
- Washington
