mod account;
mod accounts;
mod auth;
mod db;
mod event;
mod global_container;
mod principal_chain;
mod query;
mod report;
mod report_api_key;
mod report_api_keys;
mod resource;
mod state;
mod surrealdb_deserializers;
mod user;
mod value;

pub mod env;
pub mod router;

// Test builds: group under a module to make intent clear and avoid polluting root API
#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    pub use crate::account::{Account, AuthedAccount};
    pub use crate::auth::{AuthContext, AuthProvider, FixedAuthProvider, RealAuthProvider};
    pub use crate::db::{DBConnection, create_production_state};
    pub use crate::router::create_router_with_state;
    pub use crate::state::{AppState, ResourcesDbFactory};
}

use std::sync::atomic::AtomicU64;

pub(crate) use archodex_error::Result;

static NEXT_BINDING_VALUE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn next_binding() -> String {
    format!(
        "bind_{}",
        NEXT_BINDING_VALUE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}
