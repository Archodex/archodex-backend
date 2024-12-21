mod account;
mod accounts;
mod auth;
mod db;
mod env;
mod error;
mod event;
mod global_container;
mod oauth2;
mod principal_chain;
mod query;
mod report;
mod report_api_key;
mod report_api_keys;
mod resource;
mod surrealdb_deserializers;
mod user;
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
