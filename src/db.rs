use axum::{extract::Request, middleware::Next, response::Response, Extension};
use surrealdb::{
    engine::local::Db,
    opt::{capabilities::Capabilities, Config},
    Surreal,
};
use tokio::sync::OnceCell;

use crate::{auth::Principal, Result};

const DB: OnceCell<Surreal<Db>> = OnceCell::const_new();

const DYNAMODB_TABLE_PREFIX: &'static str = "archodex-service-data-";

pub(crate) fn dynamodb_resources_table_name_for_account(account_id: &str) -> String {
    format!("{DYNAMODB_TABLE_PREFIX}{account_id}-resources")
}

pub(crate) async fn db(
    Extension(principal): Extension<Principal>,
    mut req: Request,
    next: Next,
) -> Result<Response> {
    let db = DB
        .get_or_try_init(|| async {
            Surreal::new::<surrealdb::engine::local::DynamoDB>((
                DYNAMODB_TABLE_PREFIX,
                Config::default()
                    .capabilities(Capabilities::default().with_live_query_notifications(false))
                    .strict(),
            ))
            .await
        })
        .await?
        .clone();

    db.use_ns(principal.account_id())
        .use_db("resources")
        .await?;

    req.extensions_mut().insert(db);

    Ok(next.run(req).await)
}
