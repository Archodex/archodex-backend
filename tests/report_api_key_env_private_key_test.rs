// Integration tests for ReportApiKey encryption/decryption
mod common;

use archodex_backend::test_support::{ReportApiKey, User};
use common::{create_test_accounts_db, seed_test_account};

use std::sync::Once;

static INIT: Once = Once::new();

fn init() {
    INIT.call_once(|| unsafe {
        std::env::set_var(
            "ARCHODEX_API_PRIVATE_KEY",
            "000102030405060708090a0b0c0d0e0f",
        );
    });
}

/// Round-trip generate → validate with test DB and `ARCHODEX_API_PRIVATE_KEY` env var
#[tokio::test]
async fn test_report_api_key_round_trip() {
    init();

    // Setup: Create test database and seed account without an api key
    let accounts_db = create_test_accounts_db().await;
    let account = seed_test_account(&accounts_db, "1000000000", false).await;

    // Setup: Create test user and API key
    let user = User::new_for_testing("test_user_123".to_string());
    let report_api_key = ReportApiKey::new(Some("Test Key".to_string()), user);

    // Execute: Generate encrypted key value
    let key_value = report_api_key
        .generate_value("1000000000", account.salt().to_vec(), Some(&accounts_db))
        .await
        .expect("Failed to generate key value");

    // Verify: Key has correct prefix and key ID
    assert!(key_value.starts_with("archodex_report_api_key_"));
    assert!(key_value.contains(&format!("_{}_", report_api_key.id())));

    // Execute: Validate the generated key
    let (account_id, key_id) = ReportApiKey::validate_value(&key_value, Some(&accounts_db))
        .await
        .expect("Failed to validate generated key");

    // Verify: Decrypted values match original
    assert_eq!(account_id, "1000000000");
    assert_eq!(key_id, report_api_key.id());
}

#[tokio::test]
#[should_panic(expected = "ARCHODEX_API_PRIVATE_KEY environment variable must not be set")]
async fn test_panic_when_both_env_and_db_keys_exist() {
    init();

    let accounts_db = create_test_accounts_db().await;
    let account = seed_test_account(&accounts_db, "1000000000", true);

    let user = User::new_for_testing("test_user".to_string());
    let report_api_key = ReportApiKey::new(Some("Test".to_string()), user);

    report_api_key
        .generate_value(
            "1000000000",
            account.await.salt().to_vec(),
            Some(&accounts_db),
        )
        .await
        .unwrap();
}
