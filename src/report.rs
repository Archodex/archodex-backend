use core::fmt::Debug;
use std::collections::HashMap;

use axum::{Extension, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use surrealdb::{
    engine::any::Any,
    method::Query,
    sql::statements::{BeginStatement, CommitStatement, InsertStatement, UpdateStatement},
};
use tracing::{info, instrument};

use crate::{
    Result,
    account::AuthedAccount,
    db::QueryCheckFirstRealError,
    next_binding,
    resource::{ResourceId, ResourceIdPart, surrealdb_thing_from_resource_id},
    value::surrealdb_value_from_json_value,
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Principal {
    id: ResourceId,
    event: Option<String>,
}

impl From<Principal> for surrealdb::sql::Value {
    fn from(value: Principal) -> Self {
        surrealdb::sql::Object::from(HashMap::from([
            ("id", surrealdb::sql::Value::from(value.id)),
            ("event", value.event.into()),
        ]))
        .into()
    }
}

fn surrealdb_value_from_principal_chain(principal_chain: Vec<Principal>) -> surrealdb::sql::Value {
    surrealdb::sql::Array::from(
        principal_chain
            .into_iter()
            .map(surrealdb::sql::Value::from)
            .collect::<Vec<_>>(),
    )
    .into()
}

// TODO: Implement deserializer to handle unknown fields. Serde's built-in
// unknown field handling doesn't work with its flatten option.
#[derive(Debug, Deserialize, Serialize)]
struct ResourceTreeNode {
    #[serde(flatten)]
    id: ResourceIdPart,
    globally_unique: Option<bool>,
    first_seen_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    attributes: Option<serde_json::Map<String, serde_json::Value>>,
    contains: Option<Vec<ResourceTreeNode>>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Event {
    r#type: String,
    first_seen_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EventCapture {
    principals: Vec<Principal>,
    resources: Vec<ResourceId>,
    events: Vec<Event>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    resource_captures: Vec<ResourceTreeNode>,
    event_captures: Vec<EventCapture>,
}

#[instrument(skip_all)]
fn upsert_resource_tree_node<'a>(
    mut query: Query<'a, Any>,
    prefix: &mut surrealdb::sql::Array,
    resource_tree_node: ResourceTreeNode,
) -> Query<'a, Any> {
    // INSERT INTO resource (id, first_seen_at, last_seen_at) VALUES (<id>, <first_seen_at>, <last_seen_at>) ON DUPLICATE KEY UPDATE last_seen_at = <last_seen_at> RETURN NONE
    let mut resource_upsert = InsertStatement::default();
    resource_upsert.into = Some(surrealdb::sql::Table::from("resource").into());

    let mut globally_unique_prefix = surrealdb::sql::Array::new();

    let prefix = match resource_tree_node.globally_unique {
        Some(true) => &mut globally_unique_prefix,
        _ => prefix,
    };

    prefix.push(resource_tree_node.id.into());

    resource_upsert.data = surrealdb::sql::Data::ValuesExpression(vec![vec![
        ("id".into(), prefix.clone().into()),
        (
            "first_seen_at".into(),
            resource_tree_node.first_seen_at.into(),
        ),
        (
            "last_seen_at".into(),
            resource_tree_node.last_seen_at.into(),
        ),
    ]]);

    resource_upsert.update = Some(surrealdb::sql::Data::UpdateExpression(vec![(
        "last_seen_at".into(),
        surrealdb::sql::Operator::Equal,
        resource_tree_node.last_seen_at.into(),
    )]));

    resource_upsert.output = Some(surrealdb::sql::Output::None);

    info!("Resource upsert: {resource_upsert}");

    query = query.query(resource_upsert);

    if let Some(attributes) = resource_tree_node.attributes
        && !attributes.is_empty()
    {
        // UPDATE resource:<id> MERGE { attributes: <attributes> } RETURN NONE
        let mut resource_attributes_merge = UpdateStatement::default();

        resource_attributes_merge.what = vec![
            surrealdb::sql::Thing::from(("resource", surrealdb::sql::Id::from(prefix.clone())))
                .into(),
        ]
        .into();

        let mut merge_data = surrealdb::sql::Object::default();
        merge_data.insert(
            "attributes".to_string(),
            surrealdb_value_from_json_value(attributes.into()),
        );
        resource_attributes_merge.data =
            Some(surrealdb::sql::Data::MergeExpression(merge_data.into()));

        resource_attributes_merge.output = Some(surrealdb::sql::Output::None);

        info!("Resource attributes merge: {resource_attributes_merge}");

        query = query.query(resource_attributes_merge);
    }

    if let Some(children) = resource_tree_node.contains {
        for child in children {
            query = upsert_resource_tree_node(query, prefix, child);
        }
    }

    prefix.pop();

    query
}

#[allow(clippy::too_many_lines)]
#[instrument(skip_all)]
fn upsert_events(mut query: Query<'_, Any>, report: EventCapture) -> Query<'_, Any> {
    let first_seen_at = report
        .events
        .iter()
        .min_by_key(|&event| event.first_seen_at)
        .unwrap()
        .first_seen_at;

    let last_seen_at = report
        .events
        .iter()
        .max_by_key(|&event| event.last_seen_at)
        .unwrap()
        .last_seen_at;

    let principal_chain_id_var = next_binding();
    let principals_binding = next_binding();
    let first_seen_at_binding = next_binding();
    let last_seen_at_binding = next_binding();

    let statement = format!(
        "${principal_chain_id_var} = INSERT INTO principal_chain
        (id, first_seen_at, last_seen_at)
        VALUES (${principals_binding}, ${first_seen_at_binding}, ${last_seen_at_binding})
        ON DUPLICATE KEY UPDATE last_seen_at = ${last_seen_at_binding}
        RETURN id;"
    );

    let principals_value = surrealdb_value_from_principal_chain(report.principals.clone());
    let first_seen_at_value = surrealdb::sql::Datetime::from(first_seen_at);
    let last_seen_at_value = surrealdb::sql::Datetime::from(last_seen_at);

    info!(
        statement = statement,
        principal_chain_id_var = principal_chain_id_var,
        principals_binding = principals_binding,
        principals_value = tracing::field::display(&principals_value),
        principals_first_seen_binding = first_seen_at_binding,
        principals_first_seen_at_value = tracing::field::display(&first_seen_at_value),
        principals_last_seen_binding = last_seen_at_binding,
        principals_last_seen_at_value = tracing::field::display(&last_seen_at_value),
        "Principal chain insert statement"
    );

    query = query
        .query(statement)
        .bind((principals_binding, principals_value))
        .bind((first_seen_at_binding, first_seen_at_value))
        .bind((last_seen_at_binding, last_seen_at_value));

    let last_principal = report.principals.last().cloned();

    for principal in report.principals {
        let has_direct_principal_chain_value = Some(&principal) == last_principal.as_ref();
        let has_direct_principal_chain_update = if has_direct_principal_chain_value {
            ", has_direct_principal_chain = true"
        } else {
            ""
        };

        let principal_id_value = surrealdb_thing_from_resource_id(principal.id);

        for resource in &report.resources {
            let resource_id_value = surrealdb_thing_from_resource_id(resource.clone());

            for event in &report.events {
                let principal_id_binding = next_binding();
                let resource_id_binding = next_binding();
                let type_binding = next_binding();
                let has_direct_principal_chain_binding = next_binding();
                let first_seen_at_binding = next_binding();
                let last_seen_at_binding = next_binding();

                let statement = format!(
                    "INSERT RELATION INTO event
                    (in, out, type, principal_chains, has_direct_principal_chain, first_seen_at, last_seen_at)
                    VALUES (${principal_id_binding}, ${resource_id_binding}, ${type_binding}, [${principal_chain_id_var}[0].id], ${has_direct_principal_chain_binding}, ${first_seen_at_binding}, ${last_seen_at_binding})
                    ON DUPLICATE KEY UPDATE principal_chains += ${principal_chain_id_var}[0].id, last_seen_at = ${last_seen_at_binding}{has_direct_principal_chain_update}
                    RETURN NONE;"
                );

                let type_value = surrealdb::sql::Strand::from(event.r#type.as_str());
                let first_seen_at_value = surrealdb::sql::Datetime::from(event.first_seen_at);
                let last_seen_at_value = surrealdb::sql::Datetime::from(event.last_seen_at);

                info!(
                    statement = statement,
                    principal_id_binding = principal_id_binding,
                    principal_id_value = tracing::field::display(&principal_id_value),
                    resource_id_binding = resource_id_binding,
                    resource_id_value = tracing::field::display(&resource_id_value),
                    type_binding = type_binding,
                    type_value = tracing::field::display(&type_value),
                    principal_chain_id_var = principal_chain_id_var,
                    has_direct_principal_chain_binding = has_direct_principal_chain_binding,
                    has_direct_principal_chain_value = has_direct_principal_chain_value,
                    first_seen_at_binding = first_seen_at_binding,
                    first_seen_at_value = tracing::field::display(&first_seen_at_value),
                    last_seen_at_binding = last_seen_at_binding,
                    last_seen_at_value = tracing::field::display(&last_seen_at_value),
                    "Event insert statement"
                );

                query = query
                    .query(statement)
                    .bind((principal_id_binding, principal_id_value.clone()))
                    .bind((resource_id_binding, resource_id_value.clone()))
                    .bind((type_binding, type_value))
                    .bind((
                        has_direct_principal_chain_binding,
                        has_direct_principal_chain_value,
                    ))
                    .bind((first_seen_at_binding, first_seen_at_value))
                    .bind((last_seen_at_binding, last_seen_at_value));
            }
        }
    }

    query
}

#[instrument(err, skip(authed))]
pub(crate) async fn report(
    Extension(authed): Extension<AuthedAccount>,
    Json(req): Json<Request>,
) -> Result<()> {
    let db = &authed.resources_db;

    info!(
        "Received report payload: {}",
        serde_json::to_string_pretty(&req).unwrap_or_else(|e| format!("Failed to serialize: {e}"))
    );

    let mut query = db.query(BeginStatement::default());

    for resource_tree_node in req.resource_captures {
        query =
            upsert_resource_tree_node(query, &mut surrealdb::sql::Array::new(), resource_tree_node);
    }

    for events_report in req.event_captures {
        query = upsert_events(query, events_report);
    }

    query = query.query(CommitStatement::default());

    info!("Full query:\n{query:?}");

    query.await?.check_first_real_error()?;

    Ok(())
}
