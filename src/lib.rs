use surrealdb::{
    engine::local::Db,
    opt::{capabilities::Capabilities, Config},
    Surreal,
};
use tokio::sync::OnceCell;

mod error;
mod report;
mod resources;
mod signup;

pub mod router;

pub(crate) const DEMO_ACCOUNT_ID: &str = "4070221500";

static DB: OnceCell<Surreal<Db>> = OnceCell::const_new();

pub(crate) async fn db(account_id: &str) -> &'static Surreal<Db> {
    let db = DB
        .get_or_try_init(|| async {
            Surreal::new::<surrealdb::engine::local::DynamoDB>((
                "",
                Config::default()
                    .strict()
                    .capabilities(Capabilities::default().with_live_query_notifications(false)),
            ))
            .await
        })
        .await
        .expect("Failed to create SurrealDB connection");

    db.use_ns(account_id)
        .use_db("resources")
        .await
        .expect(&format!(
            "Failed to use namespace '{account_id}' database 'resources'"
        ));

    db
}
