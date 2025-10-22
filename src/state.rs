use std::sync::Arc;

use async_trait::async_trait;
use tracing::instrument;

use crate::{
    auth::AuthProvider,
    db::{DBConnection, accounts_db, resources_db},
    env::Env,
};
use archodex_error::anyhow;

/// Application state for dependency injection.
///
/// Holds database factories and auth providers for testing and production.
#[derive(Clone)]
pub struct AppState {
    /// Factory for creating per-account resources database connections
    pub resources_db_factory: Arc<dyn ResourcesDbFactory + Send + Sync>,
    /// Authentication provider for validating requests
    pub auth_provider: Arc<dyn AuthProvider>,
}

/// Factory for creating database connections.
///
/// Abstracts connection creation for testing and production use.
///
/// # Parameters
/// - `account_id`: The account ID for namespace selection
/// - `service_url`: Optional custom `SurrealDB` URL (None uses default from environment)

#[async_trait]
pub trait ResourcesDbFactory {
    async fn create_accounts_connection(&self) -> anyhow::Result<DBConnection>;

    async fn create_resources_connection(
        &self,
        account_id: &str,
        service_url: Option<&str>,
    ) -> anyhow::Result<DBConnection>;
}

/// Production implementation using global connection pools.
pub struct GlobalResourcesDbFactory;

#[async_trait]
impl ResourcesDbFactory for GlobalResourcesDbFactory {
    #[instrument(err, skip(self))]
    async fn create_accounts_connection(&self) -> anyhow::Result<DBConnection> {
        accounts_db().await.map_err(|e| anyhow::anyhow!(e))
    }

    #[instrument(err, skip(self))]
    async fn create_resources_connection(
        &self,
        account_id: &str,
        service_url: Option<&str>,
    ) -> anyhow::Result<DBConnection> {
        #[cfg(feature = "archodex-com")]
        let url = service_url
            .ok_or_else(|| anyhow::anyhow!("service_url is required for archodex-com feature"))?;

        #[cfg(not(feature = "archodex-com"))]
        let url = service_url.unwrap_or_else(|| Env::surrealdb_url());

        resources_db(url, account_id).await
    }
}
