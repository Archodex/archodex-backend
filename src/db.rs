use std::{collections::HashMap, sync::LazyLock};

use axum::{extract::Request, middleware::Next, response::Response, Extension};
use surrealdb::{
    engine::local::Db,
    opt::{capabilities::Capabilities, Config},
    Surreal,
};
use tokio::sync::{OnceCell, RwLock};

use crate::{auth::AccountAuth, env::Env, macros::*, Result};

const DB: OnceCell<Surreal<Db>> = OnceCell::const_new();

const DYNAMODB_TABLE_PREFIX: &'static str = "archodex-service-data-";

pub(crate) fn dynamodb_resources_table_name_for_account(account_id: &str) -> String {
    format!("{DYNAMODB_TABLE_PREFIX}{account_id}-resources")
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

    db.use_ns(archodex_account_id.to_string())
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

    let db = DB
        .get_or_try_init(|| async {
            let path = if Env::is_local_dev() {
                format!("{DYNAMODB_TABLE_PREFIX};profile=ddbtest")
            } else {
                DYNAMODB_TABLE_PREFIX.to_string()
            };

            Surreal::new::<surrealdb::engine::local::DynamoDB>((
                &path,
                Config::default()
                    .capabilities(Capabilities::default().with_live_query_notifications(false))
                    .strict(),
            ))
            .await
        })
        .await?
        .clone();

    db.use_ns(account_id).use_db("resources").await?;

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
