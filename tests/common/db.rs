// Database setup helpers for testing
#![allow(dead_code)]

use archodex_backend::test_support::{Account, DBConnection};
use surrealdb::{Surreal, engine::any::Any};

/// Creates an in-memory accounts database wrapped in `DBConnection`
///
/// This creates a test accounts database using `SurrealDB`'s memory engine and wraps
/// it in the `DBConnection` enum for compatibility with the application's database layer.
/// Use this function when setting up test state for integration tests.
///
/// # Examples
///
/// ```ignore
/// #[tokio::test]
/// async fn test_with_accounts_db() {
///     let accounts_db = create_test_accounts_db().await;
///     let resources_db = create_test_resources_db().await;
///
///     let app = create_test_router_with_state(accounts_db, resources_db);
///     // Test app...
/// }
/// ```
pub async fn create_test_accounts_db() -> DBConnection {
    let db: Surreal<Any> = surrealdb::engine::any::connect("mem://").await.unwrap();
    db.use_ns("archodex").use_db("accounts").await.unwrap();
    DBConnection::Concurrent(db)
}

/// Creates an in-memory resources database wrapped in `DBConnection`
///
/// This creates a test resources database using `SurrealDB`'s memory engine and wraps
/// it in the `DBConnection` enum for compatibility with the application's database layer.
/// Use this function when setting up test state for integration tests.
///
/// # Examples
///
/// ```ignore
/// #[tokio::test]
/// async fn test_with_resources_db() {
///     let accounts_db = create_test_accounts_db().await;
///     let resources_db = create_test_resources_db().await;
///
///     let app = create_test_router_with_state(accounts_db, resources_db);
///     // Test app...
/// }
/// ```
pub async fn create_test_resources_db() -> DBConnection {
    let db: Surreal<Any> = surrealdb::engine::any::connect("mem://").await.unwrap();
    db.use_ns("archodex").use_db("resources").await.unwrap();

    // Apply resources database migrations
    migrator::migrate_account_resources_database(&db)
        .await
        .expect("Failed to migrate test resources database");

    DBConnection::Concurrent(db)
}

/// Seeds a test account into the accounts database
///
/// This function creates and inserts a test account with the specified ID into the
/// accounts database. The account is created with test-appropriate defaults (random salt,
/// no real credentials). Returns the created Account for use in test assertions.
///
/// # Parameters
/// * `db` - The accounts database connection
/// * `account_id` - The ID to assign to the test account
///
/// # Examples
///
/// ```ignore
/// #[tokio::test]
/// async fn test_with_seeded_account() {
///     let accounts_db = create_test_accounts_db().await;
///     let account = seed_test_account(&accounts_db, "test_acc_123").await;
///
///     // Account is now in database and can be loaded by auth middleware
///     assert_eq!(account.id(), "test_acc_123");
/// }
/// ```
pub async fn seed_test_account(db: &DBConnection, account_id: &str) -> Account {
    use rand::Rng;

    let salt: Vec<u8> = rand::thread_rng().r#gen::<[u8; 16]>().to_vec();
    let account = Account::new_for_testing(account_id.to_string(), salt.clone());

    // Insert account into database
    if let DBConnection::Concurrent(surreal_db) = db {
        use surrealdb::sql;
        surreal_db
            .query("CREATE $account CONTENT { salt: $salt, created_at: time::now() }")
            .bind((
                "account",
                sql::Thing::from(("account", sql::Id::String(account_id.to_string()))),
            ))
            .bind(("salt", sql::Bytes::from(salt)))
            .await
            .expect("Failed to seed test account");
    } else {
        panic!("seed_test_account only works with Concurrent DBConnection");
    }

    account
}

/// Seeds a test API key into the resources database
///
/// This function creates a valid (non-revoked) API key entry in the resources database.
/// This is necessary for integration tests that use `FixedAuthProvider`, as the middleware
/// still validates that the API key exists and is not revoked.
///
/// # Parameters
/// * `db` - The resources database connection
/// * `key_id` - The key ID to create (typically 99999 for tests)
///
/// # Examples
///
/// ```ignore
/// #[tokio::test]
/// async fn test_with_api_key() {
///     let resources_db = create_test_resources_db().await;
///     seed_test_api_key(&resources_db, 99999).await;
///
///     // Now requests using FixedAuthProvider with key_id=99999 will pass validation
/// }
/// ```
pub async fn seed_test_api_key(db: &DBConnection, key_id: u32) {
    // Create a valid (non-revoked) API key entry
    // revoked_at is omitted, which makes it NONE, and type::is::none(revoked_at) will return true
    if let DBConnection::Concurrent(surreal_db) = db {
        use surrealdb::sql;

        // First, create a test user to satisfy the created_by constraint
        let test_user_id = "test_user_for_api_key";
        surreal_db
            .query("CREATE $user CONTENT { email: 'test@example.com', created_at: time::now() }")
            .bind((
                "user",
                sql::Thing::from(("user", sql::Id::String(test_user_id.to_string()))),
            ))
            .await
            .expect("Failed to create test user")
            .check()
            .expect("Test user creation failed");

        // Now create the API key with the required created_by field
        surreal_db
            .query("CREATE $record CONTENT { created_at: time::now(), created_by: $created_by }")
            .bind((
                "record",
                sql::Thing::from(("report_api_key", sql::Id::Number(i64::from(key_id)))),
            ))
            .bind((
                "created_by",
                sql::Thing::from(("user", sql::Id::String(test_user_id.to_string()))),
            ))
            .await
            .expect("Failed to execute API key seed query")
            .check()
            .expect("API key seed query returned error");
    } else {
        panic!("seed_test_api_key only works with Concurrent DBConnection");
    }
}
