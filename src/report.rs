use core::fmt::Debug;

use axum::{Extension, Json};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use surrealdb::{
    engine::local::Db,
    method::Query,
    sql::statements::{BeginStatement, CommitStatement, InsertStatement, UpdateStatement},
    Surreal,
};
use tracing::info;

use crate::{
    resource::{ResourceId, ResourceIdPart},
    value::surrealdb_value_from_json_value,
    Result,
};

impl From<ResourceIdPart> for surrealdb::sql::Value {
    fn from(value: ResourceIdPart) -> Self {
        surrealdb::sql::Array::from(vec![value.r#type, value.id]).into()
    }
}

fn surrealdb_thing_from_resource_id(value: ResourceId) -> surrealdb::sql::Value {
    surrealdb::sql::Thing::from((
        "resource",
        surrealdb::sql::Id::from(
            value
                .into_iter()
                .map(|id| surrealdb::sql::Value::from(id))
                .collect::<surrealdb::sql::Array>(),
        ),
    ))
    .into()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceTreeNode {
    #[serde(flatten)]
    id: ResourceIdPart,
    globally_unique: Option<bool>,
    first_seen_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    attributes: Option<serde_json::Map<String, serde_json::Value>>,
    contains: Option<Vec<ResourceTreeNode>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    r#type: String,
    first_seen_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EventCapture {
    principals: Vec<ResourceId>,
    resources: Vec<ResourceId>,
    events: Vec<Event>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    resource_captures: Vec<ResourceTreeNode>,
    event_captures: Vec<EventCapture>,
}

fn upsert_resource_tree_node<'a, 'b>(
    mut query: Query<'b, Db>,
    prefix: &mut surrealdb::sql::Array,
    resource_tree_node: ResourceTreeNode,
) -> Query<'b, Db> {
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

    if let Some(attributes) = resource_tree_node.attributes {
        if !attributes.is_empty() {
            // UPDATE resource:<id> MERGE { attributes: <attributes> } RETURN NONE
            let mut resource_attributes_merge = UpdateStatement::default();

            resource_attributes_merge.what = vec![surrealdb::sql::Thing::from((
                "resource",
                surrealdb::sql::Id::from(prefix.clone()),
            ))
            .into()]
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
    }

    if let Some(children) = resource_tree_node.contains {
        for child in children {
            query = upsert_resource_tree_node(query, prefix, child);
        }
    }

    let mut contains_upsert = InsertStatement::default();

    contains_upsert.relation = true;

    contains_upsert.into = Some(surrealdb::sql::Table::from("contains").into());

    let out = prefix.clone();
    prefix.pop();
    let r#in = prefix.clone();

    contains_upsert.data = surrealdb::sql::Data::ValuesExpression(vec![vec![
        (
            "in".into(),
            surrealdb::sql::Thing::from(("resource", surrealdb::sql::Id::from(r#in))).into(),
        ),
        (
            "out".into(),
            surrealdb::sql::Thing::from(("resource", surrealdb::sql::Id::from(out))).into(),
        ),
        (
            "first_seen_at".into(),
            resource_tree_node.first_seen_at.into(),
        ),
        (
            "last_seen_at".into(),
            resource_tree_node.last_seen_at.into(),
        ),
    ]]);

    contains_upsert.update = Some(surrealdb::sql::Data::UpdateExpression(vec![(
        "last_seen_at".into(),
        surrealdb::sql::Operator::Equal,
        resource_tree_node.last_seen_at.into(),
    )]));

    contains_upsert.output = Some(surrealdb::sql::Output::None);

    info!("Contains upsert: {contains_upsert}");

    query.query(contains_upsert)
}

fn upsert_events<'a, 'b>(mut query: Query<'b, Db>, report: EventCapture) -> Query<'b, Db> {
    for principal in report.principals {
        let principal_id = surrealdb_thing_from_resource_id(principal);

        for resource in &report.resources {
            let resource_id = surrealdb_thing_from_resource_id(resource.clone());

            for event in &report.events {
                let mut event_upsert = InsertStatement::default();

                event_upsert.relation = true;

                event_upsert.into = Some(surrealdb::sql::Table::from("event").into());

                event_upsert.data = surrealdb::sql::Data::ValuesExpression(vec![vec![
                    ("in".into(), principal_id.clone()),
                    ("out".into(), resource_id.clone()),
                    (
                        "type".into(),
                        surrealdb::sql::Strand::from(event.r#type.as_str()).into(),
                    ),
                    ("first_seen_at".into(), event.first_seen_at.into()),
                    ("last_seen_at".into(), event.last_seen_at.into()),
                ]]);

                event_upsert.update = Some(surrealdb::sql::Data::UpdateExpression(vec![(
                    "last_seen_at".into(),
                    surrealdb::sql::Operator::Equal,
                    event.last_seen_at.into(),
                )]));

                event_upsert.output = Some(surrealdb::sql::Output::None);

                info!("Event upsert: {event_upsert}");

                query = query.query(event_upsert)
            }
        }
    }

    query
}

#[axum::debug_handler]
pub(crate) async fn report(
    Extension(db): Extension<Surreal<Db>>,
    Json(req): Json<Request>,
) -> Result<()> {
    let mut query = db.query(BeginStatement::default());

    for resource_tree_node in req.resource_captures {
        query =
            upsert_resource_tree_node(query, &mut surrealdb::sql::Array::new(), resource_tree_node);
    }

    for events_report in req.event_captures {
        query = upsert_events(query, events_report);
    }

    query = query.query(CommitStatement::default());

    query.await?.check()?;

    Ok(())
}
