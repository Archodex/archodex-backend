use std::time::{Duration, Instant};

use aws_sdk_dynamodb::{
    error::ProvideErrorMetadata,
    operation::{
        create_table::CreateTableError::ResourceInUseException,
        update_continuous_backups::UpdateContinuousBackupsError,
    },
    types::{
        AttributeDefinition, BillingMode, KeySchemaElement, KeyType,
        PointInTimeRecoverySpecification, ScalarAttributeType, SseSpecification, SseType,
        TableStatus,
    },
    Client,
};
use axum::{Extension, Json};
use serde::Deserialize;
use surrealdb::{engine::local::Db, Surreal};
use tokio::{sync::OnceCell, time::sleep};
use tracing::{error, info, trace, warn};

use crate::{auth::Principal, db::dynamodb_resources_table_name_for_account, macros::*, Result};

static DYNAMODB_CLIENT: OnceCell<aws_sdk_dynamodb::Client> = OnceCell::const_new();

async fn ddb_client() -> &'static aws_sdk_dynamodb::Client {
    DYNAMODB_CLIENT
        .get_or_init(|| async {
            let config = aws_config::load_from_env().await;
            Client::new(&config)
        })
        .await
}

#[derive(Debug, Deserialize)]
pub(crate) struct SignupRequest {}

pub(crate) async fn signup(
    Extension(principal): Extension<Principal>,
    Extension(db): Extension<Surreal<Db>>,
    Json(_): Json<SignupRequest>,
) -> Result<()> {
    let Some(account_id) = principal.account_id() else {
        not_found!("Account is not initialized yet");
    };

    let client = ddb_client().await;
    let table_name = dynamodb_resources_table_name_for_account(&account_id.to_string());

    info!("Creating DynamoDB table {table_name}...");

    if let Err(error) = client
        .create_table()
        .table_name(&table_name)
        .attribute_definitions(
            AttributeDefinition::builder()
                .attribute_name("pk")
                .attribute_type(ScalarAttributeType::B)
                .build()?,
        )
        .attribute_definitions(
            AttributeDefinition::builder()
                .attribute_name("sk")
                .attribute_type(ScalarAttributeType::B)
                .build()?,
        )
        .key_schema(
            KeySchemaElement::builder()
                .attribute_name("pk")
                .key_type(KeyType::Hash)
                .build()?,
        )
        .key_schema(
            KeySchemaElement::builder()
                .attribute_name("sk")
                .key_type(KeyType::Range)
                .build()?,
        )
        .billing_mode(BillingMode::PayPerRequest)
        .deletion_protection_enabled(true)
        .sse_specification(
            SseSpecification::builder()
                .enabled(true)
                .sse_type(SseType::Kms)
                .kms_master_key_id("alias/ArchodexBackendCustomerDataKey")
                .build(),
        )
        .send()
        .await
    {
        match error.into_service_error() {
            ResourceInUseException(_) => conflict!("Account already exists"),
            err => bail!(err),
        };
    }

    info!("Table {table_name} created");

    info!("Waiting for table {table_name} to become available...");

    let start = Instant::now();

    loop {
        trace!("Describing table {table_name}...");

        let table_desc = client
            .describe_table()
            .table_name(&table_name)
            .send()
            .await?;

        let status = table_desc
            .table()
            .expect("Table description missing from DescribeTable response")
            .table_status()
            .expect("Table status missing from DescribeTable response");

        trace!("Table {table_name} status is {status}");

        if status == &TableStatus::Active {
            break;
        }

        ensure!(
            Instant::now().duration_since(start) <= Duration::from_secs(30),
            "Table {table_name} failed to become available within 30 seconds"
        );

        sleep(Duration::from_secs(1)).await;
    }

    info!("Table {table_name} is available");

    info!("Enabling Point In Time Recovery for table {table_name}...");

    loop {
        match client
            .update_continuous_backups()
            .table_name(&table_name)
            .point_in_time_recovery_specification(
                PointInTimeRecoverySpecification::builder()
                    .point_in_time_recovery_enabled(true)
                    .build()
                    .expect(&format!(
                        "Failed to build DynamoDB PITR specification for table {table_name}"
                    )),
            )
            .send()
            .await
        {
            Ok(_) => break,
            Err(err) => match err.into_service_error() {
                UpdateContinuousBackupsError::ContinuousBackupsUnavailableException(_) => (),
                err if err.code() == Some("UnknownOperationException") => {
                    warn!("Ignoring DynamoDB Point In Time Recovery unknown operation error, which is expected with DynamoDB Local");
                    break;
                }
                err => bail!("Failed to enable DynamoDB PITR for table {table_name}: {err:#?}"),
            },
        };

        trace!(
            "Table {table_name} is still enabling continuous backups, will retry enabling PITR..."
        );

        ensure!(
            Instant::now().duration_since(start) <= Duration::from_secs(30),
            "Table {table_name} failed to become available with PITR within 30 seconds"
        );

        sleep(Duration::from_secs(1)).await;
    }

    info!("Point In Time Recovery enabled for table {table_name}");

    info!(
        "Migrating 'resources' database for account {}...",
        account_id
    );

    while let Err(err) = migrator::migrate_account_resources_database(&db).await {
        error!("{err:#?}");
        bail!(err);
    }

    info!("Table {table_name} migrated and ready for use");

    Ok(())
}
