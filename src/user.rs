use serde::{Deserialize, Serialize};
use surrealdb::Uuid;

use crate::{
    Result,
    account::Account,
    db::{QueryCheckFirstRealError, accounts_db},
    surrealdb_deserializers,
};

#[cfg(feature = "archodex-com")]
use archodex_error::anyhow::anyhow;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct User {
    #[serde(deserialize_with = "surrealdb_deserializers::uuid::deserialize")]
    id: Uuid,
}

impl User {
    pub(crate) fn new(id: Uuid) -> Self {
        Self { id }
    }

    pub(crate) async fn ensure_user_record_exists(&self) -> Result<()> {
        accounts_db()
            .await?
            .query("UPSERT $user RETURN NONE")
            .bind(("user", surrealdb::sql::Thing::from(self)))
            .await?
            .check_first_real_error()?;

        Ok(())
    }

    #[cfg(feature = "archodex-com")]
    pub(crate) async fn has_user_account(&self) -> Result<bool> {
        #[derive(Deserialize)]
        struct HasAccountResults {
            has_user_account: bool,
        }

        Ok(accounts_db()
            .await?
            .query("SELECT COUNT(->has_access->account) > 0 AS has_user_account FROM $user")
            .bind(("user", surrealdb::sql::Thing::from(self)))
            .await?
            .check_first_real_error()?
            .take::<Option<HasAccountResults>>(0)?
            .ok_or_else(|| anyhow!("Failed to query whether user has an account"))?
            .has_user_account)
    }

    pub(crate) async fn list_accounts(&self) -> Result<Vec<Account>> {
        #[derive(Default, Deserialize)]
        struct ListAccountResults {
            accounts: Vec<Account>,
        }

        Ok(accounts_db()
            .await?
            .query("SELECT ->has_access->account.* AS accounts FROM ONLY $user")
            .bind(("user", surrealdb::sql::Thing::from(self)))
            .await?
            .check_first_real_error()?
            .take::<Option<ListAccountResults>>(0)?
            .unwrap_or_default()
            .accounts)
    }
}

impl From<&User> for surrealdb::sql::Thing {
    fn from(user: &User) -> surrealdb::sql::Thing {
        surrealdb::sql::Thing::from((
            "user",
            surrealdb::sql::Id::Uuid(surrealdb::sql::Uuid::from(user.id)),
        ))
    }
}
