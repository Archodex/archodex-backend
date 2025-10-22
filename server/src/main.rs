use anyhow::Context as _;
use archodex_backend::env::Env;
use tracing::{info, warn};

#[cfg(debug_assertions)]
const RUNTIME_STACK_SIZE: usize = 20 * 1024 * 1024; // 20MiB in debug mode
#[cfg(not(debug_assertions))]
const RUNTIME_STACK_SIZE: usize = 10 * 1024 * 1024; // 10MiB in release mode

fn setup_logging() {
    use std::io::IsTerminal;
    use tracing_subscriber::{
        filter::{EnvFilter, LevelFilter},
        fmt,
    };

    let color = std::io::stdout().is_terminal()
        && (match std::env::var("COLORTERM") {
            Ok(value) => value == "truecolor" || value == "24bit",
            _ => false,
        } || match std::env::var("TERM") {
            Ok(value) => value == "direct" || value == "truecolor",
            _ => false,
        });

    let env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();

    let fmt = fmt().with_env_filter(env_filter);

    if color {
        fmt.event_format(fmt::format().pretty())
            .with_ansi(color)
            .init();
    } else {
        fmt.with_ansi(false).init();
    }
}

/// Sets up surrealdb environment variables for configuration settings that cannot be modified through other means.
///
/// # Safety
///
/// This function is marked as `unsafe` because it modifies environment variables using `std::env::set_var()`, which is
/// inherently unsafe in multi-threaded contexts. Concurrent access to environment variables from multiple threads can
/// lead to data races and undefined behavior. See the documentation for `std::env::set_var()` for more details about
/// the safety requirements and potential issues.
///
/// This function should only be called during application initialization, before any additional threads are spawned, to
/// avoid race conditions.
unsafe fn setup_surrealdb_env_vars() {
    // Set the `SURREAL_SYNC_DATA` environment variable is to "true" if it hasn't been set already. This forces
    // SurrealDB to operate in synchronous data mode to prioritize consistency of writes over raw speed of transactions.
    //
    // This is primarily a concern when both the process and the OS die (e.g. power failure) and flushes to the OS fail
    // to reach the disk. That said, self-hosted Archodex instances using RocksDB or SurrealKV, which are the only two
    // engines this affects, should not be resource constrained to the point that flushing data to disk causes a
    // significant performance concern.
    if std::env::var("SURREAL_SYNC_DATA").is_err() {
        unsafe {
            std::env::set_var("SURREAL_SYNC_DATA", "true");
        }
    }
}

async fn wait_for_ctrl_c() {
    match tokio::signal::ctrl_c().await {
        Ok(()) => info!("Received SIGINT (Ctrl+C), initiating graceful shutdown"),
        Err(error) => {
            warn!(%error, "Failed to listen for Ctrl+C signal; waiting for SIGTERM");
            std::future::pending::<()>().await;
        }
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut terminate = match signal(SignalKind::terminate()) {
            Ok(signal) => signal,
            Err(error) => {
                warn!(%error, "Failed to listen for SIGTERM; relying on Ctrl+C handler only");
                wait_for_ctrl_c().await;
                return;
            }
        };

        tokio::select! {
            () = wait_for_ctrl_c() => {},
            _ = terminate.recv() => {
                info!("Received SIGTERM, initiating graceful shutdown");
            }
        }
    }

    #[cfg(not(unix))]
    {
        wait_for_ctrl_c().await;
    }
}

fn main() -> anyhow::Result<()> {
    // This is safe to call first thing at process start before any threads may be spawned (e.g. by tokio)
    unsafe { setup_surrealdb_env_vars() };

    setup_logging();

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(RUNTIME_STACK_SIZE)
        .build()
        .unwrap()
        .block_on(async {
            {
                migrator::migrate_accounts_database(
                    Env::accounts_surrealdb_url(),
                    Env::surrealdb_creds(),
                )
                .await
                .with_context(|| {
                    format!(
                        "Failed to migrate accounts database for URL {}",
                        Env::accounts_surrealdb_url()
                    )
                })?;
            }

            let port = Env::port();

            let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
                .await
                .unwrap_or_else(|_| panic!("Failed to listen on port {port}"));

            info!("Listening on port {port}");

            let router = archodex_backend::router::router().await;

            axum::serve(listener, router)
                .with_graceful_shutdown(shutdown_signal())
                .await?;

            anyhow::Ok(())
        })?;

    Ok(())
}
