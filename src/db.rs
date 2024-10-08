use axum::{extract::Request, middleware::Next, response::Response, Extension};
use surrealdb::{
    engine::local::Db,
    opt::{capabilities::Capabilities, Config},
    Surreal,
};
use tokio::sync::OnceCell;

use crate::{auth::Principal, Result};

const DB: OnceCell<Surreal<Db>> = OnceCell::const_new();

pub(crate) async fn db(
    Extension(principal): Extension<Principal>,
    mut req: Request,
    next: Next,
) -> Result<Response> {
    let db = DB
        .get_or_try_init(|| async {
            Surreal::new::<surrealdb::engine::local::DynamoDB>((
                "",
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
