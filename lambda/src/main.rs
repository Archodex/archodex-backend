use std::{io, thread};

use futures_lite::future;
use tokio::runtime::Builder;

fn setup_logging() {
    use tracing_subscriber::{
        filter::{EnvFilter, LevelFilter},
        fmt,
    };

    let env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();

    let fmt = fmt().with_env_filter(env_filter);

    fmt.with_ansi(false).init();
}

fn main() -> Result<(), io::Error> {
    setup_logging();

    // Create a channel used to send and receive outputs from our lambda handler. Realistically, this would be either an unbounded channel
    // or a bounded channel with a higher capacity as needed.
    let (lambda_tx, lambda_rx) = async_channel::bounded(1);

    // Create a bounded channel used to communicate our shutdown signal across threads.
    let (shutdown_tx, shutdown_rx) = async_channel::bounded(1);

    // Build a single-threaded (or multi-threaded using Builder::new_multi_thread) runtime to spawn our lambda work onto.
    let tokio_runtime = Builder::new_multi_thread()
        .thread_name("lambda-runtime")
        .thread_stack_size(10 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("build lambda runtime");

    // Run the lambda runtime worker thread to completion. The response is sent to the other "runtime" to be processed as needed.
    thread::spawn(move || {
        let router = archodex_backend::router::router();
        if let Ok(response) = tokio_runtime.block_on(lambda_http::run(router)) {
            lambda_tx
                .send_blocking(response)
                .expect("send lambda result");
        };
    });

    // Run the mock runtime to completion.
    my_runtime(move || future::block_on(app_runtime_task(lambda_rx.clone(), shutdown_tx.clone())));

    // Block the main thread until a shutdown signal is received.
    future::block_on(shutdown_rx.recv()).map_err(|err| io::Error::other(format!("{err:?}")))
}

/// A task to be ran on the custom runtime. Once a response from the lambda runtime is received then a shutdown signal
/// is sent to the main thread notifying the process to exit.
pub(crate) async fn app_runtime_task(
    lambda_rx: async_channel::Receiver<()>,
    shutdown_tx: async_channel::Sender<()>,
) {
    loop {
        // Receive the response sent by the lambda handle and process as needed.
        if let Ok(result) = lambda_rx.recv().await {
            lambda_http::tracing::debug!(?result);
            // We're ready to shutdown our app. Send the shutdown signal notifying the main thread to exit the process.
            shutdown_tx.send(()).await.expect("send shutdown signal");
            break;
        }
    }
}

/// Construct the mock runtime worker thread(s) to spawn some work onto.
fn my_runtime(func: impl Fn() + Send + 'static) {
    thread::Builder::new()
        .name("my-runtime".into())
        .spawn(func)
        .expect("spawn my_runtime worker");
}
