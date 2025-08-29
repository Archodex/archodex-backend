use anyhow::Context as _;
use axum::{
    Json,
    body::Body,
    http::{Response, StatusCode},
    response::IntoResponse,
};
use serde::Serialize;
use tracing::error;

#[derive(Debug)]
pub struct PublicError {
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
    pub fn new<S: Into<String>>(status_code: StatusCode, message: S) -> Self {
        Self {
            status_code,
            message: message.into(),
        }
    }
}

pub type Result<T> = std::result::Result<T, PublicError>;

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

        if err.is::<surrealdb::Error>() {
            match err.downcast::<surrealdb::Error>() {
                Ok(surrealdb::Error::Db(surrealdb::error::Db::Ds(msg))) => {
                    // Log SurrealDB datastore error messages directly
                    error!("SurrealDB Datastore error: {msg}");
                }
                Ok(err) => {
                    error!("SurrealDB error: {err:#?}");
                }
                Err(err) => {
                    error!("Failed to downcast SurrealDB error: {err:#?}");
                }
            }
        } else {
            error!("Unhandled error: {err:#?}");
        }

        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::INTERNAL_SERVER_ERROR
                .canonical_reason()
                .unwrap(),
        )
    }
}

#[macro_export]
macro_rules! bad_request {
        ($msg:literal $(,)?) => {
            $crate::bail!($crate::PublicError::new(
                ::axum::http::StatusCode::BAD_REQUEST,
                format!($msg),
            ))
        };
        ($fmt:expr, $($arg:tt)*) => {
            $crate::bail!($crate::PublicError::new(
                ::axum::http::StatusCode::BAD_REQUEST,
                format!($fmt, $($arg)*),
            ))
        };
    }

#[macro_export]
macro_rules! unauthorized {
    () => {
        $crate::bail!($crate::PublicError::new(
            ::axum::http::StatusCode::UNAUTHORIZED,
            "Unauthorized",
        ))
    };
}

#[macro_export]
macro_rules! forbidden {
        ($msg:literal $(,)?) => {
            $crate::bail!($crate::PublicError::new(
                ::axum::http::StatusCode::FORBIDDEN,
                format!($msg),
            ))
        };
        ($fmt:expr, $($arg:tt)*) => {
            $crate::bail!($crate::PublicError::new(
                ::axum::http::StatusCode::FORBIDDEN,
                format!($fmt, $($arg)*),
            ))
        };
    }

#[macro_export]
macro_rules! not_found {
        ($msg:literal $(,)?) => {
            $crate::bail!($crate::PublicError::new(
                ::axum::http::StatusCode::NOT_FOUND,
                format!($msg),
            ))
        };
        ($fmt:expr, $($arg:tt)*) => {
            $crate::bail!($crate::PublicError::new(
                ::axum::http::StatusCode::NOT_FOUND,
                format!($fmt, $($arg)*),
            ))
        };
    }

#[macro_export]
macro_rules! conflict {
        ($msg:literal $(,)?) => {
            $crate::bail!($crate::PublicError::new(
                ::axum::http::StatusCode::CONFLICT,
                format!($msg),
            ))
        };
        ($fmt:expr, $($arg:tt)*) => {
            $crate::bail!($crate::PublicError::new(
                ::axum::http::StatusCode::CONFLICT,
                format!($fmt, $($arg)*),
            ))
        };
    }

pub mod anyhow {
    pub use anyhow::Context;
    pub use anyhow::Error;
    pub use anyhow::Ok;
    pub use anyhow::Result;
    pub use anyhow::anyhow;

    #[macro_export]
    macro_rules! bail {
        ($msg:literal $(,)?) => {
            return Err(archodex_error::anyhow::anyhow!($msg).into())
        };
        ($err:expr $(,)?) => {
            return Err(archodex_error::anyhow::anyhow!($err).into())
        };
        ($fmt:expr, $($arg:tt)*) => {
            return Err(archodex_error::anyhow::anyhow!($fmt, $($arg)*).into())
        };
    }
    pub use bail;

    #[macro_export]
    macro_rules! ensure {
        ($cond:expr $(,)?) => {
            if !$cond {
                $crate::bail!(concat!("Condition failed: `", stringify!($cond), "`"))
            }
        };
        ($cond:expr, $msg:literal $(,)?) => {
            if !$cond {
                $crate::bail!($msg);
            }
        };
        ($cond:expr, $err:expr $(,)?) => {
            if !$cond {
                $crate::bail!($err);
            }
        };
        ($cond:expr, $fmt:expr, $($arg:tt)*) => {
            if !$cond {
                $crate::bail!($fmt, $($arg)*);
            }
        };
    }
    pub use ensure;
}
