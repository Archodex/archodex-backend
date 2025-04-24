use std::include_str;

use surrealdb::{
    engine::local::Db,
    opt::{capabilities::Capabilities, Config},
    Surreal,
};
use tracing::info;

pub async fn migrate_account_resources_database(db: &Surreal<Db>) -> Result<(), anyhow::Error> {
    const RESOURCES_SURQL: &str = include_str!("resources.surql");

    info!("Executing queries in file resources.surql...");

    db.query(RESOURCES_SURQL).await?.check()?;

    info!("Successfully completed migration");

    Ok(())
}

pub async fn migrate_accounts_database() -> Result<(), anyhow::Error> {
    const ACCOUNTS_SURQL: &str = include_str!("accounts.surql");

    info!("Executing queries in file accounts.surql...");

    let db = Surreal::new::<surrealdb::engine::local::DynamoDB>((
        "",
        Config::default()
            .capabilities(Capabilities::default().with_live_query_notifications(false))
            .strict(),
    ))
    .await?;

    db.use_ns("archodex").use_db("accounts").await?;

    db.query(ACCOUNTS_SURQL).await?.check()?;

    info!("Successfully completed migration");

    Ok(())
}
