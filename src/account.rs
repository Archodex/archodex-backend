use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use surrealdb::{Surreal, engine::any::Any};

use crate::{
    db::{db_for_customer_data_account, migrate_service_data_database},
    env::Env,
    next_binding, surrealdb_deserializers,
    user::User,
};
use archodex_error::anyhow::{self, bail};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ServiceDataLocation {
    r#type: String,
    partition: String,
    region: String,
    account_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Account {
    #[serde(deserialize_with = "surrealdb_deserializers::string::deserialize")]
    id: String,
    endpoint: String,
    service_data_surrealdb_url: Option<String>,
    #[serde(deserialize_with = "surrealdb_deserializers::bytes::deserialize")]
    salt: Vec<u8>,
    created_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct AccountPublic {
    pub(crate) id: String,
    pub(crate) endpoint: String,
}

impl From<Account> for AccountPublic {
    fn from(record: Account) -> Self {
        Self {
            id: record.id,
            endpoint: record.endpoint,
        }
    }
}

impl Account {
    pub(crate) async fn new(endpoint: String, account_id: Option<String>) -> anyhow::Result<Self> {
        let id = if let Some(account_id) = account_id {
            account_id
        } else {
            rand::thread_rng()
                .gen_range::<u64, _>(1000000000..=9999999999)
                .to_string()
        };

        #[cfg(not(feature = "archodex-com"))]
        let service_data_surrealdb_url = Some(Env::surrealdb_url().to_string());
        #[cfg(feature = "archodex-com")]
        let service_data_surrealdb_url = if endpoint == Env::endpoint() {
            Some(archodex_com::create_account_service_database(&id).await?)
        } else {
            None
        };

        if let Some(service_data_surrealdb_url) = &service_data_surrealdb_url {
            migrate_service_data_database(service_data_surrealdb_url, &id).await?;
        }

        Ok(Self {
            id,
            endpoint,
            service_data_surrealdb_url,
            salt: rand::thread_rng().r#gen::<[u8; 16]>().to_vec(),
            created_at: None,
        })
    }

    pub(crate) fn salt(&self) -> &[u8] {
        &self.salt
    }

    pub(crate) async fn surrealdb_client(&self) -> anyhow::Result<Surreal<Any>> {
        if let Some(service_data_surrealdb_url) = &self.service_data_surrealdb_url {
            db_for_customer_data_account(service_data_surrealdb_url, &self.id).await
        } else {
            bail!(
                "No service data SurrealDB URL configured for account {}",
                self.id
            );
        }
    }
}

pub(crate) trait AccountQueries<'r, C: surrealdb::Connection> {
    fn create_account_query(self, account: &Account) -> surrealdb::method::Query<'r, C>;
    fn add_account_access_for_user(
        self,
        account: &Account,
        user: &User,
    ) -> surrealdb::method::Query<'r, C>;
    fn get_account_by_id(self, account_id: String) -> surrealdb::method::Query<'r, C>;
}

impl<'r, C: surrealdb::Connection> AccountQueries<'r, C> for surrealdb::method::Query<'r, C> {
    fn create_account_query(self, account: &Account) -> surrealdb::method::Query<'r, C> {
        let account_binding = next_binding();
        let endpoint_binding = next_binding();
        let service_data_surrealdb_url_binding = next_binding();
        let salt_binding = next_binding();

        self
            .query(format!("CREATE ${account_binding} CONTENT {{ endpoint: ${endpoint_binding}, service_data_surrealdb_url: ${service_data_surrealdb_url_binding}, salt: ${salt_binding} }} RETURN NONE"))
            .bind((account_binding, surrealdb::sql::Thing::from(account)))
            .bind((endpoint_binding, account.endpoint.to_owned()))
            .bind((service_data_surrealdb_url_binding, account.service_data_surrealdb_url.to_owned()))
            .bind((salt_binding, surrealdb::sql::Bytes::from(account.salt.to_owned())))
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
}

impl From<&Account> for surrealdb::sql::Thing {
    fn from(account: &Account) -> surrealdb::sql::Thing {
        surrealdb::sql::Thing::from(("account", surrealdb::sql::Id::String(account.id.to_owned())))
    }
}
