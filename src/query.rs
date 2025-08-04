use axum::{extract::Path, Extension, Json};
use serde::{Deserialize, Serialize};
use surrealdb::{engine::any::Any, Surreal};

use crate::{
    db::{BeginReadonlyStatement, QueryCheckFirstRealError},
    event::Event,
    global_container::GlobalContainer,
    resource::Resource,
    Result,
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
    Extension(db): Extension<Surreal<Any>>,
) -> Result<Json<QueryResponse>> {
    const BEGIN: &str = "LET $resources: set<object> = []; LET $events: set<object> = [];";

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
            .query(BeginReadonlyStatement)
            .query(BEGIN)
            .query(Resource::get_all())
            .query(Event::get_all())
            .query(FINISH),

        QueryType::Secrets => {
            const SECRETS_QUERY: &str = include_str!("query_secrets.surql");

            db.query(BeginReadonlyStatement)
                .query(BEGIN)
                .query(SECRETS_QUERY)
                .query(FINISH)
        }
    };

    let mut res = query.await?.check_first_real_error()?;

    let query_response: Option<QueryResponse> = res.take(res.num_statements() - 1)?;

    Ok(Json(query_response.unwrap()))
}
