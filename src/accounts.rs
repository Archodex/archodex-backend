use std::time::{Duration, Instant};

use anyhow::Context;
use axum::{Extension, Json};
use serde::Serialize;
use surrealdb::sql::statements::{BeginStatement, CommitStatement};
use tokio::time::sleep;
use tracing::{info, trace, warn};

use crate::{
    account::{Account, AccountPublic, AccountQueries, ServiceDataLocation},
    auth::DashboardAuth,
    db::{
        accounts_db, db_for_customer_data_account, dynamodb_resources_table_name_for_account,
        QueryCheckFirstRealError,
    },
    env::Env,
    macros::*,
    Result,
};

#[derive(Serialize)]
pub(crate) struct ListAccountsResponse {
    accounts: Vec<AccountPublic>,
}

pub(crate) async fn list_accounts(
    Extension(auth): Extension<DashboardAuth>,
) -> Result<Json<ListAccountsResponse>> {
    let accounts = auth
        .principal()
        .list_accounts()
        .await?
        .into_iter()
        .map(AccountPublic::from)
        .collect();

    Ok(Json(ListAccountsResponse { accounts }))
}

async fn get_customer_data_aws_account_ids() -> anyhow::Result<Vec<String>> {
    let customer_data_ou_id = Env::customer_data_aws_account_id();

    let client = Env::aws_organizations_client().await;

    let account_list = client
        .list_accounts_for_parent()
        .parent_id(customer_data_ou_id)
        .send()
        .await
        .context("Failed to list customer data AWS accounts")?;

    Ok(account_list
        .accounts
        .ok_or_else(|| anyhow!("Response from AWS Organizations account list missing `Accounts`"))?
        .into_iter()
        .map(|account| {
            account
                .id
                .ok_or_else(|| anyhow!("Response from AWS Organizations account list missing `Id`"))
        })
        .collect::<anyhow::Result<_>>()?)
}

async fn select_customer_data_aws_account(aws_account_ids: Vec<String>) -> anyhow::Result<String> {
    use aws_sdk_cloudwatch::types::{Dimension, Metric, MetricDataQuery, MetricStat, Statistic};

    let client = Env::aws_cloudwatch_client().await;

    let mut req = client
        .get_metric_data()
        .start_time((std::time::SystemTime::now() - std::time::Duration::from_secs(10 * 60)).into())
        .end_time(std::time::SystemTime::now().into());

    for account_id in aws_account_ids {
        req = req.metric_data_queries(
            MetricDataQuery::builder()
                .id(format!("table_count_{account_id}"))
                .account_id(account_id)
                .metric_stat(
                    MetricStat::builder()
                        .period(60)
                        .stat(Statistic::Maximum.to_string())
                        .metric(
                            Metric::builder()
                                .namespace("AWS/Usage")
                                .metric_name("ResourceCount")
                                .dimensions(
                                    Dimension::builder()
                                        .name("Service")
                                        .value("DynamoDB")
                                        .build(),
                                )
                                .dimensions(
                                    Dimension::builder().name("Type").value("Resource").build(),
                                )
                                .dimensions(
                                    Dimension::builder()
                                        .name("Resource")
                                        .value("TableCount")
                                        .build(),
                                )
                                .dimensions(
                                    Dimension::builder().name("Class").value("None").build(),
                                )
                                .build(),
                        )
                        .build(),
                )
                .build(),
        );
    }

    let metric_data = req
        .send()
        .await
        .context("Failed to get number of DynamoDB tables in customer data accounts")?;

    let metrics = metric_data.metric_data_results.ok_or_else(|| {
        anyhow!("Response from CloudWatch GetMetricData missing `MetricDataResults`")
    })?;

    let table_counts = metrics
        .into_iter()
        .map(|metric| {
            let id = metric
                .id
                .ok_or_else(|| anyhow!("Metric missing `Id`"))?
                .trim_start_matches("table_count_")
                .to_owned();
            let num_tables = *metric
                .values
                .ok_or_else(|| anyhow!("Metric missing `Values`"))?
                .first()
                .unwrap_or(&0f64) as u32;

            Ok((id, num_tables))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    info!("Customer data accounts table counts: {table_counts:#?}");

    let aws_account_id = table_counts
        .into_iter()
        .min_by_key(|table_count| table_count.1)
        .expect("No AWS customer data accounts?")
        .0;

    Ok(aws_account_id)
}

async fn create_account_service_data_table(account: &Account) -> anyhow::Result<()> {
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
    };

    let aws_partition = Env::aws_partition();
    let aws_region = Env::aws_region();
    let backend_aws_account_id = Env::backend_aws_account_id();

    let archodex_account_id = account.id();
    let customer_data_aws_account_id = account
        .service_data_location()
        .ok_or(anyhow!("Account missing service data location"))?
        .account_id();

    let client = Env::aws_dynamodb_client_for_customer_data_account(
        archodex_account_id,
        customer_data_aws_account_id,
    )
    .await;

    let table_name = dynamodb_resources_table_name_for_account(&archodex_account_id.to_string());

    info!("Creating DynamoDB table {table_name}...");

    let table_arn = match client
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
        .deletion_protection_enabled(!Env::is_local_dev())
        .sse_specification(
            SseSpecification::builder()
                .enabled(true)
                .sse_type(SseType::Kms)
                .kms_master_key_id(format!("arn:aws:kms:{aws_region}:{backend_aws_account_id}:alias/ArchodexBackendCustomerDataKey"))
                .build(),
        )
        .send()
        .await
    {
        Ok(result) => result
            .table_description()
            .unwrap()
            .table_arn()
            .unwrap()
            .to_string(),
        Err(err) => match err.into_service_error() {
            ResourceInUseException(_) => conflict!("Account already exists"),
            err => bail!(err),
        },
    };

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

    info!("Adding Resource Policy to table {table_name}...");

    let policy = serde_json::to_string_pretty(&serde_json::json!({
        "Version": "2012-10-17",
        "Statement": [
            {
                "Effect": "Allow",
                "Principal": {
                    "AWS": format!("arn:{aws_partition}:iam::{backend_aws_account_id}:root")
                },
                "Action": [
                    "dynamodb:BatchGetItem",
                    "dynamodb:BatchWriteItem",
                    "dynamodb:ConditionCheckItem",
                    "dynamodb:DeleteItem",
                    "dynamodb:DeleteTable",
                    "dynamodb:DescribeTable",
                    "dynamodb:DescribeTimeToLive",
                    "dynamodb:GetItem",
                    "dynamodb:PutItem",
                    "dynamodb:Query",
                    "dynamodb:UpdateItem",
                    "dynamodb:UpdateTable",
                ],
                "Resource": "*",
                "Condition": {
                    "ArnLike": {
                        "aws:PrincipalArn": [
                            format!("arn:{aws_partition}:iam::{backend_aws_account_id}:role/ArchodexBackendAPIRole"),
                            format!("arn:{aws_partition}:iam::{backend_aws_account_id}:role/aws-reserved/sso.amazonaws.com/us-west-2/AWSReservedSSO_AdministratorAccess_*")
                        ]
                    }
                }
            }
        ]
    }))
    .with_context(|| format!("Failed to serialize Resource Policy for table {table_name}"))?;

    if !Env::is_local_dev() {
        client
            .put_resource_policy()
            .resource_arn(table_arn)
            .policy(policy)
            .send()
            .await?;

        info!("Resource Policy added to table {table_name}");
    } else {
        info!("Skipping adding Resource Policy to table {table_name} in local dev mode");
    }

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
        archodex_account_id
    );

    // We can migrate using the backend API role and the resource policy set
    // above. But the resource policy can take 30+ seconds to propagate.
    // Instead, we'll use the customer data management role to migrate the
    // database.
    let db = db_for_customer_data_account(
        customer_data_aws_account_id,
        archodex_account_id,
        Some(&Env::aws_customer_data_account_role_arn(customer_data_aws_account_id))
    )
        .await
        .with_context(|| format!("Failed to get SurrealDB client in AWS customer data account {customer_data_aws_account_id} for account {archodex_account_id}"))?;

    migrator::migrate_account_resources_database(&db)
        .await
        .with_context(|| format!("Failed to migrate 'resources' database for account {archodex_account_id} in AWS account {customer_data_aws_account_id}"))?;

    info!("Table {table_name} migrated and ready for use");

    Ok(())
}

pub(crate) async fn create_account(
    Extension(auth): Extension<DashboardAuth>,
) -> Result<Json<AccountPublic>> {
    let aws_region = Env::aws_region();
    let endpoint = Env::endpoint();

    let accounts_db = accounts_db().await?;

    let principal = auth.principal();
    principal.ensure_user_record_exists().await?;

    if principal.has_user_account().await? {
        conflict!("User already has an account");
    }

    let customer_data_aws_account_id =
        select_customer_data_aws_account(get_customer_data_aws_account_ids().await?)
            .await
            .context("Failed to select AWS account for customer data")?;

    info!(
        "Selected AWS customer account {customer_data_aws_account_id:?} for customer service data"
    );

    let customer_data_aws_account_id = if Env::is_local_dev() {
        let customer_data_aws_account_id = "098765432109".to_string();
        info!(
            "Overriding AWS customer account in local dev mode to {customer_data_aws_account_id:?}"
        );
        customer_data_aws_account_id
    } else {
        customer_data_aws_account_id
    };

    let account = Account::new(
        endpoint.to_string(),
        Some(ServiceDataLocation::new(
            aws_region.to_string(),
            customer_data_aws_account_id.clone(),
        )),
    );

    create_account_service_data_table(&account).await?;

    accounts_db
        .query(BeginStatement::default())
        .create_account_query(&account)
        .add_account_access_for_user(&account, &principal)
        .query(CommitStatement::default())
        .await?
        .check_first_real_error()?;

    Ok(Json(account.into()))
}
