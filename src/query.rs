use axum::{extract::Path, Extension, Json};
use serde::{Deserialize, Serialize};
use surrealdb::{engine::local::Db, Surreal};

use crate::{
    db::QueryCheckFirstRealError, event::Event, global_container::GlobalContainer,
    resource::Resource, Result,
};

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(super) enum QueryType {
    All,
    Secrets,
}

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct QueryResponse {
    resources: Vec<Resource>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    global_containers: Vec<GlobalContainer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    events: Option<Vec<Event>>,
}

pub(super) async fn query(
    Path((_account_id, r#type)): Path<(String, QueryType)>,
    Extension(db): Extension<Surreal<Db>>,
) -> Result<Json<QueryResponse>> {
    const BEGIN: &str = "BEGIN READONLY; $resources = []; $events = [];";

    const FINISH: &str = "{
        resources: $resources,
        events: $events,
        global_containers: fn::fetch_global_containers(
            array::concat(
                $resources.map(|$resource| $resource.id),
                $events.map(|$event| $event.in),
                $events.map(|$event| $event.out),
            ).distinct()
        ),
    };
    
    COMMIT;";

    let query = match r#type {
        QueryType::All => db
            .query(BEGIN)
            .query(Resource::get_all())
            .query(Event::get_all())
            .query(FINISH),

        QueryType::Secrets => {
            let (secrets_statement, secrets_binding) = Resource::add_resources_of_type("Secret");

            let (secret_values_statement, secret_values_binding) =
                Resource::add_resources_of_type("Secret Value");

            db.query(BEGIN)
                .query(secrets_statement)
                .bind(secrets_binding)
                .query(secret_values_statement)
                .bind(secret_values_binding)
                .query(Event::add_events_going_to_resources())
                .query(Event::add_events_going_from_resources())
                .query(FINISH)
        }
    };

    let mut res = query.await?.check_first_real_error()?;

    let query_response: Option<QueryResponse> = res.take(res.num_statements() - 1)?;

    Ok(Json(query_response.unwrap()))
}
