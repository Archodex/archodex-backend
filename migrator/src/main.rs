use std::thread;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    use tracing_subscriber::{
        filter::{EnvFilter, LevelFilter},
        fmt,
    };

    let env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();

    let fmt = fmt().with_env_filter(env_filter);

    fmt.with_ansi(false).init();

    // Build a single-threaded (or multi-threaded using Builder::new_multi_thread) runtime to spawn our work onto with a larger stack size of SurrealDB.
    let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
        .thread_name("runtime")
        .thread_stack_size(10 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("build runtime");

    // Run the lambda runtime worker thread to completion. The response is sent to the other "runtime" to be processed as needed.
    thread::spawn(move || {
        #[cfg(not(feature = "archodex-com"))]
        let (_, accounts_surrealdb_url) = (
            std::env::var("ACCOUNTS_SURREALDB_URL").expect_err(
                "ACCOUNTS_SURREALDB_URL env var should not be set in non-archodex-com builds",
            ),
            std::env::var("SURREALDB_URL").expect("Missing SURREALDB_URL env var"),
        );

        #[cfg(feature = "archodex-com")]
        let (accounts_surrealdb_url, _) = (
            std::env::var("ACCOUNTS_SURREALDB_URL")
                .expect("Missing ACCOUNTS_SURREALDB_URL env var"),
            std::env::var("SURREALDB_URL")
                .expect_err("SURREALDB_URL env var should not be set in archodex-com builds"),
        );

        let surrealdb_username = std::env::var("SURREALDB_USERNAME").ok();
        let surrealdb_password = std::env::var("SURREALDB_PASSWORD").ok();

        let surrealdb_creds = match (surrealdb_username, surrealdb_password) {
            (Some(surrealdb_username), Some(surrealdb_password)) => {
                Some(surrealdb::opt::auth::Root {
                    username: Box::leak(Box::new(surrealdb_username)),
                    password: Box::leak(Box::new(surrealdb_password)),
                })
            }
            (None, None) => None,
            _ => panic!(
                "Both SURREALDB_USERNAME and SURREALDB_PASSWORD must be set or unset together"
            ),
        };

        tokio_runtime.block_on(migrator::migrate_accounts_database(
            &accounts_surrealdb_url,
            surrealdb_creds,
        ))
    })
    .join()
    .expect("runtime thread should join successfully")
}
