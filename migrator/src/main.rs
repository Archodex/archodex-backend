use std::thread;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    use tracing_subscriber::{filter::EnvFilter, fmt};

    let env_filter = if let Ok(rust_log) = std::env::var("RUST_LOG") {
        EnvFilter::builder().parse_lossy(rust_log)
    } else {
        EnvFilter::builder()
            .parse("surrealdb_core::kvs::dynamodb=debug,info")
            .unwrap()
    };

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
    thread::spawn(move || tokio_runtime.block_on(migrator::migrate_accounts_database()))
        .join()
        .expect("runtime thread should join successfully")
}
