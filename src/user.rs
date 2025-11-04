use serde::{Deserialize, Serialize};
use surrealdb::Uuid;
use tracing::instrument;

use crate::{
    Result,
    account::Account,
    db::{DBConnection, QueryCheckFirstRealError},
    surrealdb_deserializers,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct User {
    #[serde(deserialize_with = "surrealdb_deserializers::uuid::deserialize")]
    id: Uuid,
}

impl User {
    pub(crate) fn new(id: Uuid) -> Self {
        Self { id }
    }

    #[instrument(err)]
    pub(crate) async fn ensure_user_record_exists(&self, accounts_db: &DBConnection) -> Result<()> {
        accounts_db
            .query("UPSERT $user RETURN NONE")
            .bind(("user", surrealdb::sql::Thing::from(self)))
            .await?
            .check_first_real_error()?;

        Ok(())
    }

    #[cfg(feature = "archodex-com")]
    #[instrument(err)]
    pub(crate) async fn next_account_id(&self, accounts_db: &DBConnection) -> Result<String> {
        use crate::env::Env;
        use archodex_error::{anyhow::anyhow, conflict};
        use rand::Rng as _;
        use tracing::info;

        #[derive(Deserialize)]
        struct NumUserAccountsResults {
            num_user_accounts: u32,
        }

        let NumUserAccountsResults { num_user_accounts } = accounts_db
            .query("SELECT COUNT(->has_access->(account WHERE deleted_at IS NONE)) AS num_user_accounts FROM ONLY $user")
            .bind(("user", surrealdb::sql::Thing::from(self)))
            .await?
            .check_first_real_error()?
            .take::<Option<NumUserAccountsResults>>(0)?
            .ok_or_else(|| anyhow!("Failed to query whether user has an account"))?;

        info!(num_user_accounts, "Retrieved number of accounts for user");

        if num_user_accounts >= Env::user_account_limit() {
            conflict!("User account limit exceeded");
        }

        let account_id = rand::thread_rng()
            .gen_range::<u64, _>(1_000_000_000..=9_999_999_999)
            .to_string();

        info!(account_id, "Generated new account ID");

        Ok(account_id)
    }

    #[instrument(err)]
    pub(crate) async fn list_accounts(&self, accounts_db: &DBConnection) -> Result<Vec<Account>> {
        #[derive(Default, Deserialize)]
        struct ListAccountResults {
            accounts: Vec<Account>,
        }

        Ok(accounts_db
            .query("SELECT ->has_access->(account WHERE deleted_at IS NONE).* AS accounts FROM ONLY $user")
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

#[cfg(any(test, feature = "test-support"))]
impl User {
    /// Creates a User for testing with a deterministic ID derived from a string.
    #[allow(clippy::needless_pass_by_value)]
    #[must_use]
    pub fn new_for_testing(id_string: String) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        // Generate a deterministic UUID from the string for testing
        let mut hasher = DefaultHasher::new();
        id_string.hash(&mut hasher);
        let hash = hasher.finish();

        // Create a UUID v4 from the hash (this is deterministic for testing)
        #[allow(clippy::cast_possible_truncation)]
        let uuid_bytes = [
            (hash >> 56) as u8,
            (hash >> 48) as u8,
            (hash >> 40) as u8,
            (hash >> 32) as u8,
            (hash >> 24) as u8,
            (hash >> 16) as u8,
            (hash >> 8) as u8,
            hash as u8,
            0x12,
            0x34,
            0x56,
            0x78, // Deterministic filler
            0x9a,
            0xbc,
            0xde,
            0xf0,
        ];

        Self {
            id: Uuid::from_bytes(uuid_bytes),
        }
    }
}
