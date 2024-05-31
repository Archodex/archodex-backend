use std::time::{Duration, Instant};

use anyhow::anyhow;
use aws_sdk_dynamodb::{
    operation::create_table::CreateTableError::ResourceInUseException,
    types::{
        AttributeDefinition, BillingMode, KeySchemaElement, KeyType, ScalarAttributeType,
        TableStatus,
    },
    Client,
};
use axum::{http::StatusCode, Json};
use serde::Deserialize;
use tokio::{sync::OnceCell, time::sleep};
use tracing::{info, trace};

use crate::{
    db,
    error::{Error, IntoError},
    DEMO_ACCOUNT_ID,
};

static DYNAMODB_CLIENT: OnceCell<aws_sdk_dynamodb::Client> = OnceCell::const_new();

async fn get_client() -> &'static aws_sdk_dynamodb::Client {
    DYNAMODB_CLIENT
        .get_or_init(|| async {
            let config = aws_config::load_from_env().await;
            Client::new(&config)
        })
        .await
}

#[derive(Debug, Deserialize)]
pub(crate) struct SignupRequest {}

pub async fn signup(Json(_): Json<SignupRequest>) -> Result<(), Error> {
    let client = get_client().await;
    let table_name = format!("{}-resources", DEMO_ACCOUNT_ID);

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
        .send()
        .await
    {
        return match error.into_service_error() {
            ResourceInUseException(_) => Err(StatusCode::CONFLICT.into_error()),
            error => Err(error.into()),
        };
    }

    info!("Waiting for table {table_name} to become active...");

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
            info!("Table {table_name} is now active");
            break;
        }

        if Instant::now().duration_since(start) > Duration::from_secs(30) {
            return Err(
                anyhow!("Table {table_name} failed to become available within 30 seconds").into(),
            );
        }

        sleep(Duration::from_secs(1)).await;
    }

    info!("Migrating 'resources' database for account {DEMO_ACCOUNT_ID}");

    migrator::migrate_account_resources_database(db(DEMO_ACCOUNT_ID).await).await?;

    Ok(())
}
