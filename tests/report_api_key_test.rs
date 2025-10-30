// Integration tests for ReportApiKey encryption/decryption
mod common;

use archodex_backend::test_support::{ReportApiKey, User, report_api_key::proto};
use common::{create_test_accounts_db, seed_test_account};

/// Round-trip generate → validate with test DB
#[tokio::test]
async fn test_report_api_key_round_trip() {
    // Setup: Create test database and seed account
    let accounts_db = create_test_accounts_db().await;
    let account = seed_test_account(&accounts_db, "1000000000", true).await;

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

/// Out-of-range account ID in encrypted payload
#[tokio::test]
async fn test_report_api_key_invalid_account_id_out_of_range() {
    // Setup: Create test database and seed account with invalid ID
    let accounts_db = create_test_accounts_db().await;
    let account = seed_test_account(&accounts_db, "999999999", true).await; // Below range

    let user = User::new_for_testing("test_user_123".to_string());
    let report_api_key = ReportApiKey::new(Some("Test Key".to_string()), user);

    // Execute: Generate key with low account ID
    let key_value = report_api_key
        .generate_value("999999999", account.salt().to_vec(), Some(&accounts_db))
        .await
        .expect("Failed to generate key value");

    // Execute: Validate should fail due to account ID range check
    let result = ReportApiKey::validate_value(&key_value, Some(&accounts_db)).await;

    // Verify: Validation fails with account ID out of range error
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Account ID is out of range")
    );
}

/// Invalid base64 in key value
#[tokio::test]
async fn test_report_api_key_invalid_base64() {
    // Setup: Create test database
    let accounts_db = create_test_accounts_db().await;

    // Execute: Try to validate key with invalid base64
    let result = ReportApiKey::validate_value(
        "archodex_report_api_key_123456_!!!invalid!!!",
        Some(&accounts_db),
    )
    .await;

    // Verify: Decoding fails
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Failed to base64 decode")
    );
}

/// Invalid salt length (AAD validation)
#[tokio::test]
async fn test_report_api_key_invalid_salt_length() {
    use base64::prelude::*;
    use prost::Message;

    // Setup: Create test database
    let accounts_db = create_test_accounts_db().await;

    // Setup: Create protobuf with wrong salt length
    let proto_key = proto::ReportApiKey {
        version: 1,
        #[cfg(feature = "archodex-com")]
        endpoint: Some("test.archodex.com".to_string()),
        #[cfg(not(feature = "archodex-com"))]
        endpoint: None,
        account_salt: vec![0u8; 15], // WRONG: Should be 16 bytes
        nonce: vec![0u8; 12],
        encrypted_contents: vec![0u8; 32],
    };

    let key_value = format!(
        "archodex_report_api_key_123456_{}",
        BASE64_STANDARD.encode(proto_key.encode_to_vec())
    );

    // Execute: Validate key with invalid salt
    let result = ReportApiKey::validate_value(&key_value, Some(&accounts_db)).await;

    // Verify: Validation fails
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Account salt is not 16 bytes long")
    );
}

/// Test 5: Corrupted ciphertext detection
#[tokio::test]
async fn test_report_api_key_corrupted_ciphertext() {
    use base64::prelude::*;
    use prost::Message;

    // Setup: Create test database
    let accounts_db = create_test_accounts_db().await;
    let account = seed_test_account(&accounts_db, "1234567890", true).await;

    // Setup: Create valid protobuf with corrupted encrypted contents
    let proto_key = proto::ReportApiKey {
        version: 1,
        #[cfg(feature = "archodex-com")]
        endpoint: Some("test.archodex.com".to_string()),
        #[cfg(not(feature = "archodex-com"))]
        endpoint: None,
        account_salt: account.salt().to_vec(),
        nonce: vec![0u8; 12], // Valid nonce length for AES-GCM
        encrypted_contents: vec![1, 2, 3, 4, 5], // Corrupted/invalid ciphertext
    };

    let key_value = format!(
        "archodex_report_api_key_123456_{}",
        BASE64_STANDARD.encode(proto_key.encode_to_vec())
    );

    // Execute: Validate corrupted key
    let result = ReportApiKey::validate_value(&key_value, Some(&accounts_db)).await;

    // Verify: Decryption fails
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Failed to decrypt")
    );
}
