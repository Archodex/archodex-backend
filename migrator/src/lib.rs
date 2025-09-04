use std::include_str;

use anyhow::{Context as _, bail};
use surrealdb::{
    Surreal,
    engine::any::Any,
    opt::{Config, capabilities::Capabilities},
};
use tracing::info;

pub async fn migrate_account_resources_database(db: &Surreal<Any>) -> Result<(), anyhow::Error> {
    const RESOURCES_SURQL: &str = include_str!("resources.surql");

    info!("Executing queries in file resources.surql...");

    db.query(RESOURCES_SURQL).await?.check()?;

    info!("Successfully completed migration");

    Ok(())
}

pub async fn migrate_accounts_database(
    surrealdb_url: &str,
    creds: Option<surrealdb::opt::auth::Root<'_>>,
) -> Result<(), anyhow::Error> {
    const ACCOUNTS_SURQL: &str = include_str!("accounts.surql");

    info!("Executing queries in file accounts.surql...");

    let res = surrealdb::engine::any::connect((
        surrealdb_url,
        Config::default()
            .capabilities(Capabilities::default().with_live_query_notifications(false))
            .strict(),
    ))
    .await;

    if let Err(surrealdb::Error::Api(surrealdb::error::Api::Ws(err))) = &res {
        bail!(
            "Failed to connect to SurrealDB at {surrealdb_url}. Please ensure that the SurrealDB instance is running and accessible. ({err})"
        );
    }

    let db = res?;

    if let Some(creds) = creds {
        db.signin(creds)
            .await
            .context("Failed to sign in to accounts database")?;
    }

    #[cfg(not(feature = "archodex-com"))]
    {
        db.query("DEFINE NAMESPACE IF NOT EXISTS archodex;")
            .await?
            .check()?;

        db.use_ns("archodex").await?;

        db.query("DEFINE DATABASE IF NOT EXISTS accounts;")
            .await?
            .check()?;

        db.use_db("accounts").await?;
    }

    #[cfg(feature = "archodex-com")]
    {
        db.use_ns("archodex").use_db("accounts").await?;
    }

    db.query(ACCOUNTS_SURQL).await?.check()?;

    info!("Successfully completed migration");

    Ok(())
}
