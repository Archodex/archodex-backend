use axum::{middleware, routing::*, Router};
use tower::ServiceBuilder;
use tower_http::trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::{auth::auth, db::db, report, signup};

pub fn router() -> Router {
    Router::new()
        .route("/signup", post(signup::signup))
        .route("/report", post(report::report))
        .layer(
            ServiceBuilder::new()
                .layer(middleware::from_fn(auth))
                .layer(middleware::from_fn(db)),
        )
        .layer(middleware::from_fn(auth))
        .route("/health", get(|| async { "Ok" }))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
}
