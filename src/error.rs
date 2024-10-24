use anyhow::Context;
use axum::{
    body::Body,
    http::{Response, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Serialize;
use tracing::error;

#[derive(Debug)]
pub(super) struct PublicError {
    status_code: axum::http::StatusCode,
    message: String,
}

// Generates strings like "409 Conflict: Account already exists"
impl std::fmt::Display for PublicError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.status_code, self.message)
    }
}

impl PublicError {
    pub(super) fn new<S: Into<String>>(status_code: StatusCode, message: S) -> Self {
        Self {
            status_code,
            message: message.into(),
        }
    }
}

pub(super) type Result<T> = std::result::Result<T, PublicError>;

// Tell axum how to convert `Error` into a response.
impl IntoResponse for PublicError {
    fn into_response(self) -> Response<Body> {
        #[derive(Serialize)]
        struct PublicErrorMessage {
            message: String,
        }

        (
            self.status_code,
            Json(PublicErrorMessage {
                message: self.message,
            }),
        )
            .into_response()
    }
}

// This enables using `?` on functions that return `Result<_, anyhow::Error>` to turn them into
// `Result<_, Error>`. That way you don't need to do that manually.
impl<E> From<E> for PublicError
where
    E: Into<anyhow::Error>,
{
    fn from(value: E) -> Self {
        let err: anyhow::Error = value.into();

        if err.is::<PublicError>() {
            return match err.downcast().context("Failed to downcast PublicError") {
                Ok(err) => err,
                Err(err) => PublicError::from(err),
            };
        }

        error!("{err:#?}");

        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::INTERNAL_SERVER_ERROR
                .canonical_reason()
                .unwrap(),
        )
    }
}

pub(super) mod macros {
    #[allow(unused_macros)]
    macro_rules! bad_request {
        ($msg:literal $(,)?) => {
            bail!($crate::error::PublicError::new(
                ::axum::http::StatusCode::BAD_REQUEST,
                format!($msg),
            ))
        };
        ($fmt:expr, $($arg:tt)*) => {
            bail!($crate::error::PublicError::new(
                ::axum::http::StatusCode::BAD_REQUEST,
                format!($fmt, $($arg)*),
            ))
        };
    }
    #[allow(unused_imports)]
    pub(crate) use bad_request;

    #[allow(unused_macros)]
    macro_rules! not_found {
        ($msg:literal $(,)?) => {
            bail!($crate::error::PublicError::new(
                ::axum::http::StatusCode::NOT_FOUND,
                format!($msg),
            ))
        };
        ($fmt:expr, $($arg:tt)*) => {
            bail!($crate::error::PublicError::new(
                ::axum::http::StatusCode::NOT_FOUND,
                format!($fmt, $($arg)*),
            ))
        };
    }
    #[allow(unused_imports)]
    pub(crate) use not_found;

    #[allow(unused_macros)]
    macro_rules! conflict {
        ($msg:literal $(,)?) => {
            bail!($crate::error::PublicError::new(
                ::axum::http::StatusCode::CONFLICT,
                format!($msg),
            ))
        };
        ($fmt:expr, $($arg:tt)*) => {
            bail!($crate::error::PublicError::new(
                ::axum::http::StatusCode::CONFLICT,
                format!($fmt, $($arg)*),
            ))
        };
    }
    #[allow(unused_imports)]
    pub(crate) use conflict;

    // Re-implement anyhow macros to work with above error types
    pub(crate) use anyhow::anyhow;

    macro_rules! bail {
        ($msg:literal $(,)?) => {
            return Err(anyhow!($msg).into())
        };
        ($err:expr $(,)?) => {
            return Err(anyhow!($err).into())
        };
        ($fmt:expr, $($arg:tt)*) => {
            return Err(anyhow!($fmt, $($arg)*).into())
        };
    }
    pub(crate) use bail;

    #[allow(unused_macros)]
    macro_rules! ensure {
        ($cond:expr $(,)?) => {
            if !$cond {
                bail!(concat!("Condition failed: `", stringify!($cond), "`"))
            }
        };
        ($cond:expr, $msg:literal $(,)?) => {
            if !$cond {
                bail!($msg);
            }
        };
        ($cond:expr, $err:expr $(,)?) => {
            if !$cond {
                bail!($err);
            }
        };
        ($cond:expr, $fmt:expr, $($arg:tt)*) => {
            if !$cond {
                bail!($fmt, $($arg)*);
            }
        };
    }
    #[allow(unused_imports)]
    pub(crate) use ensure;
}
