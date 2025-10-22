use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use axum::{
    Extension,
    extract::{Path, Request, State},
    middleware::Next,
    response::Response,
};
use surrealdb::{
    Surreal,
    engine::any::Any,
    opt::{Config, capabilities::Capabilities},
};
use tokio::sync::{OnceCell, RwLock};
use tracing::{info, instrument, warn};

use crate::{
    Result,
    account::{Account, AccountQueries, AuthedAccount},
    auth::{DashboardAuth, ReportApiKeyAuth},
    env::Env,
    state::{AppState, GlobalResourcesDbFactory},
};
use archodex_error::{
    anyhow::{self, Context as _},
    not_found,
};

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

#[instrument(err)]
pub(crate) async fn migrate_service_data_database(
    service_data_surrealdb_url: &str,
    archodex_account_id: &str,
) -> anyhow::Result<()> {
    info!("Migrating service data 'resources' database...");

    // We can migrate using the backend API role and the resource policy set
    // above. But the resource policy can take 30+ seconds to propagate.
    // Instead, we'll use the customer data management role to migrate the
    // database.
    let db = resources_db(service_data_surrealdb_url, archodex_account_id)
        .await
        .context("Failed to get SurrealDB client")?;

    #[cfg(not(feature = "archodex-com"))]
    db.query("DEFINE DATABASE resources;")
        .await?
        .check()
        .context("Failed to define 'resources' SurrealDB database")?;

    migrator::migrate_account_resources_database(&db)
        .await
        .context("Failed to migrate 'resources' database")?;

    info!("Service data SurrealDB Database 'resources' migrated and ready for use");

    Ok(())
}

#[cfg(feature = "rocksdb")]
#[derive(PartialEq)]
enum ArchodexSurrealDatabase {
    Accounts,
    Resources,
}

#[cfg(feature = "rocksdb")]
struct NonconcurrentDBState {
    connection: Surreal<Any>,
    current_database: ArchodexSurrealDatabase,
}

#[cfg(feature = "rocksdb")]
#[instrument(err)]
async fn get_nonconcurrent_db_connection(
    url: &str,
) -> anyhow::Result<&'static tokio::sync::Mutex<NonconcurrentDBState>> {
    use tokio::sync::Mutex;

    static NONCONCURRENT_DB: OnceCell<Mutex<NonconcurrentDBState>> = OnceCell::const_new();

    NONCONCURRENT_DB
        .get_or_try_init(|| async {
            let db = surrealdb::engine::any::connect((
                url,
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

            anyhow::Ok(Mutex::new(NonconcurrentDBState { connection: db, current_database: ArchodexSurrealDatabase::Accounts }))
        })
        .await
}

#[instrument(err)]
async fn get_concurrent_db_connection(url: &str) -> anyhow::Result<Surreal<Any>> {
    static ACCOUNTS_DB: OnceCell<Surreal<Any>> = OnceCell::const_new();

    Ok(ACCOUNTS_DB
        .get_or_try_init(|| async {
            let db = surrealdb::engine::any::connect((
                url,
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

pub enum DBConnection {
    #[cfg(feature = "rocksdb")]
    Nonconcurrent(tokio::sync::MappedMutexGuard<'static, Surreal<Any>>),
    Concurrent(Surreal<Any>),
}

impl Clone for DBConnection {
    fn clone(&self) -> Self {
        match self {
            #[cfg(feature = "rocksdb")]
            DBConnection::Nonconcurrent(_) => {
                panic!(
                    "Cannot clone Nonconcurrent DBConnection (contains MutexGuard). This should never happen in normal operation as Nonconcurrent connections should not be used with Axum Extensions."
                )
            }
            DBConnection::Concurrent(db) => DBConnection::Concurrent(db.clone()),
        }
    }
}

impl std::ops::Deref for DBConnection {
    type Target = Surreal<Any>;

    fn deref(&self) -> &Self::Target {
        match self {
            #[cfg(feature = "rocksdb")]
            DBConnection::Nonconcurrent(db) => db,
            DBConnection::Concurrent(db) => db,
        }
    }
}

#[instrument(err)]
pub(crate) async fn accounts_db() -> Result<DBConnection> {
    #[cfg(feature = "archodex-com")]
    let surrealdb_url = Env::accounts_surrealdb_url();
    #[cfg(not(feature = "archodex-com"))]
    let surrealdb_url = Env::surrealdb_url();

    #[cfg(feature = "rocksdb")]
    if surrealdb_url.starts_with("rocksdb:") {
        let connection = get_nonconcurrent_db_connection(surrealdb_url).await?;
        let mut db_state = connection.lock().await;

        if db_state.current_database != ArchodexSurrealDatabase::Accounts {
            db_state.connection.use_db("accounts").await?;
            db_state.current_database = ArchodexSurrealDatabase::Accounts;
        }

        return Ok(DBConnection::Nonconcurrent(
            tokio::sync::MutexGuard::try_map(db_state, |state| Some(&mut state.connection))
                .unwrap_or_else(|_| unreachable!()),
        ));
    }

    Ok(DBConnection::Concurrent(
        get_concurrent_db_connection(surrealdb_url).await?,
    ))
}

#[instrument(err)]
pub(crate) async fn resources_db(
    service_data_surrealdb_url: &str,
    account_id: &str,
) -> anyhow::Result<DBConnection> {
    static DBS_BY_URL: LazyLock<RwLock<HashMap<String, Surreal<Any>>>> =
        LazyLock::new(|| RwLock::new(HashMap::new()));

    #[cfg(feature = "rocksdb")]
    if service_data_surrealdb_url.starts_with("rocksdb:") {
        let connection = get_nonconcurrent_db_connection(service_data_surrealdb_url).await?;
        let mut db_state = connection.lock().await;

        if db_state.current_database != ArchodexSurrealDatabase::Resources {
            db_state.connection.use_db("resources").await?;
            db_state.current_database = ArchodexSurrealDatabase::Resources;
        }

        return Ok(DBConnection::Nonconcurrent(
            tokio::sync::MutexGuard::try_map(db_state, |state| Some(&mut state.connection))
                .unwrap_or_else(|_| unreachable!()),
        ));
    }

    let dbs_by_url = DBS_BY_URL.read().await;

    let db = if let Some(db) = dbs_by_url.get(service_data_surrealdb_url) {
        db.clone()
    } else {
        drop(dbs_by_url);

        let mut dbs_by_url = DBS_BY_URL.write().await;

        if let Some(db) = dbs_by_url.get(service_data_surrealdb_url) {
            db.clone()
        } else {
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
    };

    if let Some(creds) = Env::surrealdb_creds() {
        db.signin(creds)
            .await
            .with_context(|| format!("Failed to sign in to SurrealDB instance {service_data_surrealdb_url} with SURREALDB_USERNAME and SURREALDB_PASSWORD environment values"))?;
    }

    let namespace = if cfg!(feature = "archodex-com") {
        format!("a{account_id}")
    } else {
        "archodex".to_string()
    };

    db.use_ns(namespace).use_db("resources").await?;

    Ok(DBConnection::Concurrent(db))
}

/// Creates production `AppState` with global database connections.
#[instrument]
pub async fn create_production_state() -> AppState {
    let resources_db_factory = Arc::new(GlobalResourcesDbFactory);
    let auth_provider = Arc::new(crate::auth::RealAuthProvider);

    AppState {
        resources_db_factory,
        auth_provider,
    }
}

#[instrument(err, skip_all)]
pub(crate) async fn dashboard_auth_account(
    State(state): State<AppState>,
    Extension(auth): Extension<DashboardAuth>,
    Path(params): Path<HashMap<String, String>>,
    mut req: Request,
    next: Next,
) -> Result<Response> {
    let account_id = params
        .get("account_id")
        .expect(":account_id should be in path for dashboard account authentication");

    auth.validate_account_access(account_id).await?;

    let accounts_db = state
        .resources_db_factory
        .create_accounts_connection()
        .await?;
    let account = accounts_db
        .get_account_by_id(account_id.to_owned())
        .await?
        .check_first_real_error()?
        .take::<Option<Account>>(0)
        .with_context(|| format!("Failed to get record for account ID {account_id:?}"))?;

    let Some(account) = account else {
        not_found!("Account not found");
    };

    #[cfg(feature = "archodex-com")]
    let resources_db = state
        .resources_db_factory
        .create_resources_connection(account.id(), account.service_data_surrealdb_url())
        .await?;

    #[cfg(not(feature = "archodex-com"))]
    let resources_db = state
        .resources_db_factory
        .create_resources_connection(account_id, None)
        .await?;

    let authed = AuthedAccount {
        account,
        resources_db,
    };

    req.extensions_mut().insert(authed);

    Ok(next.run(req).await)
}

#[instrument(err, skip_all)]
pub(crate) async fn report_api_key_account(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response> {
    let auth_context = state
        .auth_provider
        .authenticate(req.headers())
        .await
        .context("Authentication failed")?;

    let accounts_db = state
        .resources_db_factory
        .create_accounts_connection()
        .await?;
    let account = accounts_db
        .get_account_by_id(auth_context.account_id.clone())
        .await?
        .check_first_real_error()?
        .take::<Option<Account>>(0)
        .context("Failed to get account record")?;

    let Some(account) = account else {
        not_found!("Account not found");
    };

    #[cfg(feature = "archodex-com")]
    let resources_db = state
        .resources_db_factory
        .create_resources_connection(account.id(), account.service_data_surrealdb_url())
        .await?;

    #[cfg(not(feature = "archodex-com"))]
    let resources_db = state
        .resources_db_factory
        .create_resources_connection(&auth_context.account_id, None)
        .await?;

    let auth =
        ReportApiKeyAuth::from_credentials(auth_context.account_id.clone(), auth_context.key_id);
    auth.validate_account_access(&resources_db).await?;

    let authed = AuthedAccount {
        account,
        resources_db,
    };

    req.extensions_mut().insert(authed);

    Ok(next.run(req).await)
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
