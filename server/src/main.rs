mod assets;
mod db;
mod routes;

use axum::Router;
use routes::AppState;
use shuttle_shared_db::Postgres;

#[shuttle_runtime::main]
async fn main(#[Postgres] pool: sqlx::PgPool) -> shuttle_axum::ShuttleAxum {
    db::migrate(&pool)
        .await
        .expect("database migrations should succeed");
    db::seed_fixture_if_empty(&pool)
        .await
        .expect("fixture seed should succeed");

    let state = AppState { pool };
    let router = Router::new()
        .merge(routes::api_router(state))
        .fallback(assets::static_handler);

    Ok(router.into())
}
