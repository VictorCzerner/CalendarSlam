# CalendarSlam

Rust full-stack browser game for drafting a perfect tennis player and simulating the four Grand Slams.

## Structure

- `shared`: DTOs and enums shared by the front and back.
- `server`: Axum + sqlx + Shuttle API, Postgres migrations, embedded front assets.
- `app`: Yew + Trunk WASM front.
- `data/fixture.sql`: fixture seed used until the real Phase 1 dataset is loaded.

## Setup

```powershell
rustup target add wasm32-unknown-unknown
cargo install trunk
cargo install cargo-shuttle
```

## Local build

```powershell
cd app
trunk build --release
cd ..
cargo build
```

The Shuttle app runs migrations and seeds `data/fixture.sql` when `editions` is empty.

## Deploy

```powershell
cd app
trunk build --release
cd ..
cargo shuttle deploy --working-directory server
```
