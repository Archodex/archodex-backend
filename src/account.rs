use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use crate::{
    db::{DBConnection, migrate_service_data_database, resources_db},
    env::Env,
    next_binding, surrealdb_deserializers,
    user::User,
};
use archodex_error::anyhow;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Account {
    #[serde(deserialize_with = "surrealdb_deserializers::string::deserialize")]
    id: String,
    #[cfg(feature = "archodex-com")]
    endpoint: String,
    #[cfg(feature = "archodex-com")]
    service_data_surrealdb_url: Option<String>,
    #[serde(deserialize_with = "surrealdb_deserializers::bytes::deserialize")]
    salt: Vec<u8>,
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
        let service_data_surrealdb_url = Env::surrealdb_url();

        migrate_service_data_database(service_data_surrealdb_url, &id).await?;

        Ok(Self {
            id,
            salt: rand::thread_rng().r#gen::<[u8; 16]>().to_vec(),
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

    pub(crate) async fn resources_db(&self) -> anyhow::Result<DBConnection> {
        #[cfg(not(feature = "archodex-com"))]
        let service_data_surrealdb_url = Env::surrealdb_url();
        #[cfg(feature = "archodex-com")]
        let Some(service_data_surrealdb_url) = &self.service_data_surrealdb_url else {
            use archodex_error::anyhow::bail;

            bail!(
                "No service data SurrealDB URL configured for account {}",
                self.id
            );
        };

        resources_db(service_data_surrealdb_url, &self.id).await
    }
}

pub(crate) trait AccountQueries<'r, C: surrealdb::Connection> {
    fn create_account_query(
        self,
        account: &Account,
        principal: &User,
    ) -> surrealdb::method::Query<'r, C>;
    fn add_account_access_for_user(
        self,
        account: &Account,
        user: &User,
    ) -> surrealdb::method::Query<'r, C>;
    fn get_account_by_id(self, account_id: String) -> surrealdb::method::Query<'r, C>;
    fn delete_account_query(
        self,
        account: &Account,
        principal: &User,
    ) -> surrealdb::method::Query<'r, C>;
}

impl<'r, C: surrealdb::Connection> AccountQueries<'r, C> for surrealdb::method::Query<'r, C> {
    fn create_account_query(
        self,
        account: &Account,
        principal: &User,
    ) -> surrealdb::method::Query<'r, C> {
        let account_binding = next_binding();
        let endpoint_binding = next_binding();
        let service_data_surrealdb_url_binding = next_binding();
        let salt_binding = next_binding();
        let created_by_binding = next_binding();

        #[cfg(not(feature = "archodex-com"))]
        let (endpoint_value, service_data_surrealdb_url_value) =
            (Option::<String>::None, Option::<String>::None);
        #[cfg(feature = "archodex-com")]
        let (endpoint_value, service_data_surrealdb_url_value) = (
            account.endpoint.clone(),
            account.service_data_surrealdb_url.clone(),
        );

        self
            .query(format!("CREATE ${account_binding} CONTENT {{ endpoint: ${endpoint_binding}, service_data_surrealdb_url: ${service_data_surrealdb_url_binding}, salt: ${salt_binding}, created_by: ${created_by_binding} }} RETURN NONE"))
            .bind((account_binding, surrealdb::sql::Thing::from(account)))
            .bind((endpoint_binding, endpoint_value))
            .bind((service_data_surrealdb_url_binding, service_data_surrealdb_url_value))
            .bind((salt_binding, surrealdb::sql::Bytes::from(account.salt.clone())))
            .bind((created_by_binding, surrealdb::sql::Thing::from(principal)))
    }

    fn add_account_access_for_user(
        self,
        account: &Account,
        user: &User,
    ) -> surrealdb::method::Query<'r, C> {
        let user_binding = next_binding();
        let account_binding = next_binding();

        self.query(format!(
            "RELATE ${user_binding}->has_access->${account_binding} RETURN NONE"
        ))
        .bind((user_binding, surrealdb::sql::Thing::from(user)))
        .bind((account_binding, surrealdb::sql::Thing::from(account)))
    }

    fn get_account_by_id(self, account_id: String) -> surrealdb::method::Query<'r, C> {
        let account_binding = next_binding();

        self.query(format!("SELECT * FROM ONLY ${account_binding}"))
            .bind((
                account_binding,
                surrealdb::sql::Thing::from(("account", surrealdb::sql::Id::String(account_id))),
            ))
    }

    fn delete_account_query(
        self,
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
