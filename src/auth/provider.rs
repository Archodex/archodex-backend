use async_trait::async_trait;
use axum::http::{HeaderMap, header::AUTHORIZATION};
use std::fmt::Debug;
use tracing::instrument;

use archodex_error::anyhow::{self, Context as _};

use crate::{report_api_key::ReportApiKey, state::ResourcesDbFactory};

/// Authentication context containing validated account and key IDs.
#[derive(Clone, Debug)]
pub struct AuthContext {
    pub account_id: String,
    pub key_id: u32,
}

/// Authentication provider trait for validating requests.
///
/// Abstracts authentication to allow production validation and test injection.
#[async_trait]
pub trait AuthProvider: Send + Sync + Debug + 'static {
    /// Validates authentication credentials and returns context.
    async fn authenticate(
        &self,
        headers: &HeaderMap,
        resources_db_factory: &dyn ResourcesDbFactory,
    ) -> anyhow::Result<AuthContext>;
}

/// Production authentication using API key validation.
#[derive(Clone, Debug)]
pub struct RealAuthProvider;

#[async_trait]
impl AuthProvider for RealAuthProvider {
    #[instrument(err, skip_all)]
    async fn authenticate(
        &self,
        headers: &HeaderMap,
        resources_db_factory: &dyn ResourcesDbFactory,
    ) -> anyhow::Result<AuthContext> {
        let header_value = headers
            .get(AUTHORIZATION)
            .context("Missing Authorization header")?;

        let header_str = header_value
            .to_str()
            .context("Failed to parse Authorization header")?;

        let accounts_db = resources_db_factory.create_accounts_connection().await?;
        let (account_id, key_id) = ReportApiKey::validate_value(header_str, Some(&accounts_db))
            .await
            .context("Failed to validate API key")?;

        Ok(AuthContext { account_id, key_id })
    }
}

/// Test authentication that returns pre-configured context.
///
/// # Usage
/// ```rust,ignore
/// let auth_provider = Arc::new(FixedAuthProvider::new("test_account_123", 99999));
/// let state = AppState { auth_provider, /* ... */ };
/// ```
#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Debug)]
pub struct FixedAuthProvider {
    context: AuthContext,
}

#[cfg(any(test, feature = "test-support"))]
impl FixedAuthProvider {
    /// Creates a new `FixedAuthProvider` with pre-configured authentication context.
    pub fn new(account_id: impl Into<String>, key_id: u32) -> Self {
        Self {
            context: AuthContext {
                account_id: account_id.into(),
                key_id,
            },
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
#[async_trait]
impl AuthProvider for FixedAuthProvider {
    async fn authenticate(
        &self,
        _headers: &HeaderMap,
        _resources_db_factory: &dyn ResourcesDbFactory,
    ) -> anyhow::Result<AuthContext> {
        Ok(self.context.clone())
    }
}
