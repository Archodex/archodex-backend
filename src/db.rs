use std::{collections::HashMap, sync::LazyLock};

use anyhow::Context;
use axum::{extract::Request, middleware::Next, response::Response, Extension};
use surrealdb::{
    engine::local::Db,
    opt::{capabilities::Capabilities, Config},
    sql::statements::CommitStatement,
    Surreal,
};
use tokio::sync::{OnceCell, RwLock};
use tracing::warn;

use crate::{
    account::{Account, AccountQueries},
    auth::AccountAuth,
    env::Env,
    macros::*,
    Result,
};

pub(crate) const DYNAMODB_TABLE_PREFIX: &'static str = "archodex-service-data-";

#[derive(Default)]
pub(crate) struct BeginReadonlyStatement;

impl surrealdb::opt::IntoQuery for BeginReadonlyStatement {
    fn into_query(self) -> surrealdb::Result<Vec<surrealdb::sql::Statement>> {
        let mut begin = surrealdb::sql::statements::BeginStatement::default();
        begin.readonly = true;
        Ok(vec![surrealdb::sql::Statement::Begin(begin)])
    }
}

pub(crate) fn dynamodb_resources_table_name_for_account(account_id: &str) -> String {
    format!("{DYNAMODB_TABLE_PREFIX}a{account_id}-resources")
}

pub(crate) async fn db_for_customer_data_account(
    customer_data_aws_account_id: &str,
    archodex_account_id: &str,
    role_arn: Option<&str>,
) -> anyhow::Result<Surreal<Db>> {
    static DBS_BY_AWS_ACCOUNT_ID: LazyLock<RwLock<HashMap<String, Surreal<Db>>>> =
        LazyLock::new(|| RwLock::new(HashMap::new()));

    let dbs_by_aws_account_id = DBS_BY_AWS_ACCOUNT_ID.read().await;

    let db = if let Some(db) = dbs_by_aws_account_id.get(customer_data_aws_account_id) {
        db.clone()
    } else {
        drop(dbs_by_aws_account_id);

        let mut dbs_by_aws_account_id = DBS_BY_AWS_ACCOUNT_ID.write().await;

        match dbs_by_aws_account_id.get(customer_data_aws_account_id) {
            Some(db) => db.clone(),
            None => {
                let path = if Env::is_local_dev() {
                    format!("{DYNAMODB_TABLE_PREFIX};profile=ddbtest")
                } else {
                    let mut path = format!(
                        "arn:{aws_partition}:dynamodb:{aws_region}:{customer_data_aws_account_id}:table/{DYNAMODB_TABLE_PREFIX}",
                        aws_partition = Env::aws_partition(),
                        aws_region = Env::aws_region(),
                    );

                    if let Some(role_arn) = role_arn {
                        path.push_str(";role_arn=");
                        path.push_str(role_arn);
                    }

                    path
                };

                let db = Surreal::new::<surrealdb::engine::local::DynamoDB>((
                    path,
                    Config::default()
                        .capabilities(Capabilities::default().with_live_query_notifications(false))
                        .strict(),
                ))
                .await?;

                dbs_by_aws_account_id.insert(customer_data_aws_account_id.to_string(), db.clone());

                db
            }
        }
    };

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
        .query(BeginReadonlyStatement::default())
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

pub(crate) async fn accounts_db() -> anyhow::Result<Surreal<Db>> {
    static ACCOUNTS_DB: OnceCell<Surreal<Db>> = OnceCell::const_new();

    Ok(ACCOUNTS_DB
        .get_or_try_init(|| async {
            let path = if Env::is_local_dev() {
                ";profile=ddbtest"
            } else {
                ""
            };

            let db = Surreal::new::<surrealdb::engine::local::DynamoDB>((
                path,
                Config::default()
                    .capabilities(Capabilities::default().with_live_query_notifications(false))
                    .strict(),
            ))
            .await?;

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
