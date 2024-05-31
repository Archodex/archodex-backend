use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use tracing::error;

// Make our own error that wraps `anyhow::Error`.
//pub(crate) struct Error(anyhow::Error);
pub(crate) struct Error {
    error: Option<anyhow::Error>,
    status_code: StatusCode,
}
/*pub(crate) enum Error {
    StatusCode(StatusCode),
    Error(anyhow::Error),
}*/

// Tell axum how to convert `Error` into a response.
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        if let Some(error) = self.error {
            error!("{error}");
        }

        self.status_code.into_response()
    }
}

// This enables using `?` on functions that return `Result<_, anyhow::Error>` to turn them into
// `Result<_, Error>`. That way you don't need to do that manually.
impl<E> From<E> for Error
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Error {
            error: Some(err.into()),
            status_code: StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

pub(crate) trait IntoError {
    fn into_error(&self) -> Error;
}

impl IntoError for StatusCode {
    fn into_error(&self) -> Error {
        Error {
            error: None,
            status_code: self.to_owned(),
        }
    }
}
