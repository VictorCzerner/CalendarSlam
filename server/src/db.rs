use sqlx::{Executor, PgPool};

pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}

pub async fn seed_fixture_if_empty(pool: &PgPool) -> Result<(), sqlx::Error> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM editions")
        .fetch_one(pool)
        .await?;

    if count == 0 {
        pool.execute(include_str!("../../data/fixture.sql")).await?;
    }

    Ok(())
}
