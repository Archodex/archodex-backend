use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use surrealdb::sql::statements::{BeginStatement, CommitStatement};
use tracing::instrument;

use crate::{
    db::{DBConnection, migrate_service_data_database},
    env::Env,
    next_binding, surrealdb_deserializers,
    user::User,
};
use archodex_error::anyhow;

// pub(crate) in production, but re-exported as pub in test_support module
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Account {
    #[serde(deserialize_with = "surrealdb_deserializers::string::deserialize")]
    id: String,
    #[cfg(feature = "archodex-com")]
    endpoint: String,
    #[cfg(feature = "archodex-com")]
    service_data_surrealdb_url: Option<String>,
    #[serde(deserialize_with = "surrealdb_deserializers::bytes::deserialize")]
    salt: Vec<u8>,
    #[cfg(not(feature = "archodex-com"))]
    #[serde(
        default,
        deserialize_with = "surrealdb_deserializers::bytes::deserialize_optional"
    )]
    api_private_key: Option<Vec<u8>>,
    created_at: Option<DateTime<Utc>>,
    created_by: Option<User>,
    deleted_at: Option<DateTime<Utc>>,
    deleted_by: Option<User>,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct AccountPublic {
    pub(crate) id: String,
    #[cfg(feature = "archodex-com")]
    pub(crate) endpoint: String,
}

impl From<Account> for AccountPublic {
    fn from(record: Account) -> Self {
        Self {
            id: record.id,
            #[cfg(feature = "archodex-com")]
            endpoint: record.endpoint,
        }
    }
}

impl Account {
    #[cfg(feature = "archodex-com")]
    #[instrument(err)]
    pub(crate) async fn new(endpoint: String, id: String, principal: User) -> anyhow::Result<Self> {
        let service_data_surrealdb_url = if endpoint == Env::endpoint() {
            let service_data_surrealdb_url =
                archodex_com::create_account_service_database(&id).await?;
            migrate_service_data_database(&service_data_surrealdb_url, &id).await?;
            Some(service_data_surrealdb_url)
        } else {
            None
        };

        Ok(Self {
            id,
            endpoint,
            service_data_surrealdb_url,
            salt: rand::thread_rng().r#gen::<[u8; 16]>().to_vec(),
            created_at: None,
            created_by: Some(principal),
            deleted_at: None,
            deleted_by: None,
        })
    }

    #[cfg(not(feature = "archodex-com"))]
    #[instrument(err)]
    pub(crate) async fn new(id: String, principal: User) -> anyhow::Result<Self> {
        use tracing::info;

        let service_data_surrealdb_url = Env::surrealdb_url();

        migrate_service_data_database(service_data_surrealdb_url, &id).await?;

        let api_private_key = if std::env::var("ARCHODEX_API_PRIVATE_KEY").is_ok() {
            info!(
                "API Private Key value found in ARCHODEX_API_PRIVATE_KEY environment variable, will not generate and store a key in the database"
            );
            None
        } else {
            info!(
                "API Private Key value was not found in ARCHODEX_API_PRIVATE_KEY environment variable, generating a new key and storing it in the database"
            );
            Some(rand::thread_rng().r#gen::<[u8; 16]>().to_vec())
        };

        Ok(Self {
            id,
            salt: rand::thread_rng().r#gen::<[u8; 16]>().to_vec(),
            api_private_key,
            created_at: None,
            created_by: Some(principal),
            deleted_at: None,
            deleted_by: None,
        })
    }

    #[cfg(feature = "archodex-com")]
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    #[cfg(feature = "archodex-com")]
    pub(crate) fn service_data_surrealdb_url(&self) -> Option<&str> {
        self.service_data_surrealdb_url.as_deref()
    }

    pub(crate) fn salt(&self) -> &[u8] {
        &self.salt
    }

    /// Creates an Account for testing, bypassing normal creation flow.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn new_for_testing(id: String, salt: Vec<u8>) -> Self {
        Self {
            id,
            #[cfg(feature = "archodex-com")]
            endpoint: "test.archodex.com".to_string(),
            #[cfg(feature = "archodex-com")]
            service_data_surrealdb_url: None,
            salt,
            #[cfg(not(feature = "archodex-com"))]
            api_private_key: None,
            created_at: None,
            created_by: None,
            deleted_at: None,
            deleted_by: None,
        }
    }
}

pub(crate) trait AccountQueries<'r, C: surrealdb::Connection> {
    fn create_account_query(
        &'r self,
        account: &Account,
        principal: &User,
    ) -> surrealdb::method::Query<'r, C>;
    fn get_account_by_id(&'r self, account_id: String) -> surrealdb::method::Query<'r, C>;
    fn delete_account_query(
        &'r self,
        account: &Account,
        principal: &User,
    ) -> surrealdb::method::Query<'r, C>;
}

impl<'r, C: surrealdb::Connection> AccountQueries<'r, C> for surrealdb::Surreal<C> {
    fn create_account_query(
        &'r self,
        account: &Account,
        principal: &User,
    ) -> surrealdb::method::Query<'r, C> {
        let account_binding = next_binding();
        let endpoint_binding = next_binding();
        let service_data_surrealdb_url_binding = next_binding();
        let salt_binding = next_binding();
        let api_private_key_binding = next_binding();
        let created_by_binding = next_binding();

        #[cfg(not(feature = "archodex-com"))]
        let (endpoint_value, service_data_surrealdb_url_value, api_private_key_value) = (
            Option::<String>::None,
            Option::<String>::None,
            account
                .api_private_key
                .clone()
                .map(surrealdb::sql::Bytes::from),
        );
        #[cfg(feature = "archodex-com")]
        let (endpoint_value, service_data_surrealdb_url_value, api_private_key_value) = (
            account.endpoint.clone(),
            account.service_data_surrealdb_url.clone(),
            Option::<surrealdb::sql::Bytes>::None,
        );

        let query = self
            .query(BeginStatement::default())
            .query(format!("CREATE ${account_binding} CONTENT {{ endpoint: ${endpoint_binding}, service_data_surrealdb_url: ${service_data_surrealdb_url_binding}, salt: ${salt_binding}, api_private_key: ${api_private_key_binding}, created_by: ${created_by_binding} }} RETURN NONE"))
            .bind((account_binding, surrealdb::sql::Thing::from(account)))
            .bind((endpoint_binding, endpoint_value))
            .bind((service_data_surrealdb_url_binding, service_data_surrealdb_url_value))
            .bind((salt_binding, surrealdb::sql::Bytes::from(account.salt.clone())))
            .bind((api_private_key_binding, api_private_key_value))
            .bind((created_by_binding, surrealdb::sql::Thing::from(principal)));

        let user_binding = next_binding();
        let account_binding = next_binding();

        query
            .query(format!(
                "RELATE ${user_binding}->has_access->${account_binding} RETURN NONE"
            ))
            .bind((user_binding, surrealdb::sql::Thing::from(principal)))
            .bind((account_binding, surrealdb::sql::Thing::from(account)))
            .query(CommitStatement::default())
    }

    fn get_account_by_id(&'r self, account_id: String) -> surrealdb::method::Query<'r, C> {
        let account_binding = next_binding();

        self.query(format!("SELECT * FROM ONLY ${account_binding}"))
            .bind((
                account_binding,
                surrealdb::sql::Thing::from(("account", surrealdb::sql::Id::String(account_id))),
            ))
    }

    fn delete_account_query(
        &'r self,
        account: &Account,
        principal: &User,
    ) -> surrealdb::method::Query<'r, C> {
        let account_binding = next_binding();
        let deleted_by_binding = next_binding();

        self.query(format!("UPDATE ${account_binding} CONTENT {{ deleted_at: time::now(), deleted_by: ${deleted_by_binding} }}"))
            .bind((
                account_binding,
                surrealdb::sql::Thing::from(account)
            ))
            .bind((deleted_by_binding, surrealdb::sql::Thing::from(principal)))
    }
}

impl From<&Account> for surrealdb::sql::Thing {
    fn from(account: &Account) -> surrealdb::sql::Thing {
        surrealdb::sql::Thing::from(("account", surrealdb::sql::Id::String(account.id.clone())))
    }
}

/// Authenticated account with its resources database connection.
///
/// Created by auth middleware after validating credentials and loading account data.
#[derive(Clone)]
pub struct AuthedAccount {
    pub account: Account,
    pub resources_db: DBConnection,
}
