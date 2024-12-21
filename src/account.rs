use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use surrealdb::{engine::local::Db, Surreal};

use crate::{
    db::db_for_customer_data_account, env::Env, macros::*, next_binding, surrealdb_deserializers,
    user::User,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ServiceDataLocation {
    r#type: String,
    partition: String,
    region: String,
    account_id: String,
}

impl ServiceDataLocation {
    pub(crate) fn new(region: String, account_id: String) -> Self {
        Self {
            r#type: "dynamodb".to_string(),
            partition: "aws".to_string(),
            region,
            account_id,
        }
    }

    pub(crate) fn account_id(&self) -> &str {
        &self.account_id
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Account {
    #[serde(deserialize_with = "surrealdb_deserializers::string::deserialize")]
    id: String,
    endpoint: String,
    service_data_location: Option<ServiceDataLocation>,
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
    pub(crate) fn new(
        endpoint: String,
        service_data_location: Option<ServiceDataLocation>,
    ) -> Self {
        Self {
            id: if Env::is_local_dev() {
                "1000000001".to_string()
            } else {
                rand::thread_rng()
                    .gen_range::<u64, _>(1000000000..=9999999999)
                    .to_string()
            },
            endpoint,
            service_data_location,
            salt: rand::thread_rng().gen::<[u8; 16]>().to_vec(),
            created_at: None,
        }
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn service_data_location(&self) -> Option<&ServiceDataLocation> {
        self.service_data_location.as_ref()
    }

    pub(crate) fn salt(&self) -> &[u8] {
        &self.salt
    }

    pub(crate) async fn surrealdb_client(&self) -> anyhow::Result<Surreal<Db>> {
        let Some(service_data_location) = &self.service_data_location else {
            bail!("Account instance missing service data location when attempting to create SurrealDB client");
        };

        ensure!(
            service_data_location.r#type == "dynamodb",
            "Unsupported service data location type ({type}) when constructing SurrealDB client",
            type = service_data_location.r#type
        );

        db_for_customer_data_account(&service_data_location.account_id, &self.id, None).await
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
        let service_data_location_binding = next_binding();
        let salt_binding = next_binding();

        self
            .query(format!("CREATE ${account_binding} CONTENT {{ endpoint: ${endpoint_binding}, service_data_location: ${service_data_location_binding}, salt: ${salt_binding} }} RETURN NONE"))
            .bind((account_binding, surrealdb::sql::Thing::from(account)))
            .bind((endpoint_binding, account.endpoint.to_owned()))
            .bind((service_data_location_binding, account.service_data_location.to_owned()))
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
