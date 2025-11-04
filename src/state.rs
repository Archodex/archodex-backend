use std::{fmt::Debug, sync::Arc};

use async_trait::async_trait;
use tracing::instrument;

#[cfg(not(feature = "archodex-com"))]
use crate::env::Env;
use crate::{
    auth::AuthProvider,
    db::{DBConnection, accounts_db, resources_db},
};

use archodex_error::anyhow;

/// Application state for dependency injection.
///
/// Holds database factories and auth providers for testing and production.
#[derive(Debug, Clone)]
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
pub trait ResourcesDbFactory: Send + Sync + Debug {
    async fn create_accounts_connection(&self) -> anyhow::Result<DBConnection>;

    async fn create_resources_connection(
        &self,
        account_id: &str,
        service_url: Option<&str>,
    ) -> anyhow::Result<DBConnection>;
}

/// Production implementation using global connection pools.
#[derive(Debug)]
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
            .ok_or_else(|| anyhow::anyhow!("service_url is required for archodex-com builds"))?;

        #[cfg(not(feature = "archodex-com"))]
        let url = {
            if service_url.is_some() {
                unreachable!("service_url should not be provided in non-archodex-com builds")
            }
            Env::surrealdb_url()
        };

        resources_db(url, account_id).await
    }
}
