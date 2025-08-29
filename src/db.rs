use std::{collections::HashMap, sync::LazyLock};

use axum::{Extension, extract::Request, middleware::Next, response::Response};
use surrealdb::{
    Surreal,
    engine::any::Any,
    opt::{Config, capabilities::Capabilities},
    sql::statements::CommitStatement,
};
use tokio::sync::{OnceCell, RwLock};
use tracing::{info, warn};

use crate::{
    Result,
    account::{Account, AccountQueries},
    auth::AccountAuth,
    env::Env,
};
use archodex_error::anyhow::{self, Context as _, anyhow, bail};

#[derive(Default)]
pub(crate) struct BeginReadonlyStatement;

impl surrealdb::opt::IntoQuery for BeginReadonlyStatement {
    fn into_query(self) -> surrealdb::Result<Vec<surrealdb::sql::Statement>> {
        let begin = {
            #[cfg(not(feature = "archodex-com"))]
            {
                surrealdb::sql::statements::BeginStatement::default()
            }
            #[cfg(feature = "archodex-com")]
            {
                archodex_com::begin_readonly_statement()
            }
        };

        Ok(vec![surrealdb::sql::Statement::Begin(begin)])
    }
}

pub(crate) async fn migrate_service_data_database(
    service_data_surrealdb_url: &str,
    archodex_account_id: &str,
) -> anyhow::Result<()> {
    info!(
        "Migrating 'resources' database for account {archodex_account_id} at URL {service_data_surrealdb_url}...",
    );

    // We can migrate using the backend API role and the resource policy set
    // above. But the resource policy can take 30+ seconds to propagate.
    // Instead, we'll use the customer data management role to migrate the
    // database.
    let db = db_for_customer_data_account(
        service_data_surrealdb_url,
        archodex_account_id,
    )
        .await
        .with_context(|| format!("Failed to get SurrealDB client for URL {service_data_surrealdb_url} for account {archodex_account_id}"))?;

    migrator::migrate_account_resources_database(&db)
        .await
        .with_context(|| format!("Failed to migrate 'resources' database for URL {service_data_surrealdb_url} for account {archodex_account_id}"))?;

    info!(
        "SurrealDB Database at {service_data_surrealdb_url} for account {archodex_account_id} migrated and ready for use"
    );

    Ok(())
}

pub(crate) async fn db_for_customer_data_account(
    service_data_surrealdb_url: &str,
    archodex_account_id: &str,
) -> anyhow::Result<Surreal<Any>> {
    static DBS_BY_URL: LazyLock<RwLock<HashMap<String, Surreal<Any>>>> =
        LazyLock::new(|| RwLock::new(HashMap::new()));

    let dbs_by_url = DBS_BY_URL.read().await;

    let db = if let Some(db) = dbs_by_url.get(service_data_surrealdb_url) {
        db.clone()
    } else {
        drop(dbs_by_url);

        let mut dbs_by_url = DBS_BY_URL.write().await;

        match dbs_by_url.get(service_data_surrealdb_url) {
            Some(db) => db.clone(),
            None => {
                let db = surrealdb::engine::any::connect((
                    service_data_surrealdb_url,
                    Config::default()
                        .capabilities(Capabilities::default().with_live_query_notifications(false))
                        .strict(),
                ))
                .await?;

                dbs_by_url.insert(service_data_surrealdb_url.to_string(), db.clone());

                db
            }
        }
    };

    if let Some(creds) = Env::surrealdb_creds() {
        db.signin(creds)
            .await
            .with_context(|| format!("Failed to sign in to SurrealDB instance {service_data_surrealdb_url} with SURREALDB_USERNAME and SURREALDB_PASSWORD environment values"))?;
    }

    db.use_ns(format!("a{archodex_account_id}"))
        .use_db("resources")
        .await?;

    Ok(db)
}

pub(crate) async fn db<A: AccountAuth>(
    Extension(auth): Extension<A>,
    mut req: Request,
    next: Next,
) -> Result<Response> {
    let Some(account_id) = auth.account_id() else {
        bail!("Missing account ID in auth extension");
    };

    let account = accounts_db()
        .await?
        .query(BeginReadonlyStatement)
        .get_account_by_id(account_id.to_owned())
        .query(CommitStatement::default())
        .await?
        .check_first_real_error()?
        .take::<Option<Account>>(0)
        .with_context(|| format!("Failed to get record for account ID {account_id:?}"))?
        .ok_or_else(|| anyhow!("Account record not found for ID {account_id:?}"))?;

    let db = account.surrealdb_client().await?;

    auth.validate(&db).await?;

    req.extensions_mut().insert(db);

    Ok(next.run(req).await)
}

pub(crate) async fn accounts_db() -> anyhow::Result<Surreal<Any>> {
    static ACCOUNTS_DB: OnceCell<Surreal<Any>> = OnceCell::const_new();

    Ok(ACCOUNTS_DB
        .get_or_try_init(|| async {
            let db = surrealdb::engine::any::connect((
                #[cfg(feature = "archodex-com")]
                Env::accounts_surrealdb_url(),
                #[cfg(not(feature = "archodex-com"))]
                Env::surrealdb_url(),
                Config::default()
                    .capabilities(Capabilities::default().with_live_query_notifications(false))
                    .strict(),
            ))
            .await?;

            if let Some(creds) = Env::surrealdb_creds() {
                db.signin(creds)
                    .await
                    .context("Failed to sign in to SurrealDB with SURREALDB_USERNAME and SURREALDB_PASSWORD environment values")?;
            }

            db.use_ns("archodex").use_db("accounts").await?;

            anyhow::Ok(db)
        })
        .await?
        .clone())
}

// Like surrealdb::Response::check, but skips over QueryNotExecuted errors.
// QueryNotExecuted errors are returned for all statements in a transaction
// other than the statement that caused the error. If a transaction fails after
// the first statement, the normal `check()` method will return QueryNotExecuted
// instead of the true cause of the error.
pub(crate) trait QueryCheckFirstRealError {
    #[allow(clippy::result_large_err)]
    fn check_first_real_error(self) -> surrealdb::Result<Self>
    where
        Self: Sized;
}

impl QueryCheckFirstRealError for surrealdb::Response {
    fn check_first_real_error(mut self) -> surrealdb::Result<Self> {
        let errors = self.take_errors();

        if errors.is_empty() {
            return Ok(self);
        }

        if let Some((_, err)) = errors
            .into_iter()
            .filter(|(_, result)| {
                !matches!(
                    result,
                    surrealdb::Error::Db(surrealdb::error::Db::QueryNotExecuted)
                )
            })
            .min_by_key(|(query_num, _)| *query_num)
        {
            return Err(err);
        }

        warn!("Only QueryNotExecuted errors found in response, which shouldn't happen");

        Err(surrealdb::Error::Db(surrealdb::error::Db::QueryNotExecuted))
    }
}
