mod auth;
mod contains;
mod db;
mod error;
mod event;
mod global_container;
mod query;
mod report;
mod resource;
mod signup;
mod value;

pub mod router;

use std::sync::atomic::AtomicU64;

pub(crate) use error::macros;
pub(crate) use error::*;

static NEXT_BINDING_VALUE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn next_binding() -> String {
    format!(
        "bind_{}",
        NEXT_BINDING_VALUE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}
