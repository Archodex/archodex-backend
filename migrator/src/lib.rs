use std::include_str;

use surrealdb::{engine::local::Db, Surreal};
use tracing::info;

const RESOURCES_SURQL: &'static str = include_str!("resources.surql");

pub async fn migrate_account_resources_database(db: &Surreal<Db>) -> Result<(), anyhow::Error> {
    info!("Executing queries in file resources.surql...");

    db.query(RESOURCES_SURQL).await?.check()?;

    info!("Successfully completed migration");

    Ok(())
}
