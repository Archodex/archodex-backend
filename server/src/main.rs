use anyhow::Context as _;
use archodex_backend::env::Env;
use tracing::info;

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
    };
}

fn main() -> anyhow::Result<()> {
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

            axum::serve(listener, archodex_backend::router::router()).await?;

            anyhow::Ok(())
        })?;

    unreachable!("Tokio runtime completed unexpectedly");
}
