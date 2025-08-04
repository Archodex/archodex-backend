use std::collections::HashMap;

use axum::{extract::Query, Extension, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use surrealdb::{engine::any::Any, Surreal};

use archodex_error::{anyhow, bad_request, bail, ensure, not_found};

use crate::{db::QueryCheckFirstRealError, resource::ResourceId};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PrincipalChainIdPart {
    pub(crate) id: ResourceId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) event: Option<String>,
}

impl From<PrincipalChainIdPart> for surrealdb::sql::Value {
    fn from(value: PrincipalChainIdPart) -> Self {
        surrealdb::sql::Object::from(HashMap::from([
            ("id", value.id.into()),
            ("event", value.event.into()),
        ]))
        .into()
    }
}

impl TryFrom<surrealdb::sql::Object> for PrincipalChainIdPart {
    type Error = anyhow::Error;

    fn try_from(mut value: surrealdb::sql::Object) -> Result<Self, Self::Error> {
        let id = match value.remove("id") {
            Some(id) => id,
            None => bail!("PrincipalChainIdPart::try_from::<surrealdb::sql::Object> called with an object missing the `id` key"),
        };

        let id = match id {
            surrealdb::sql::Value::Array(id) => ResourceId::try_from(id)?,
            _ => bail!("PrincipalChainIdPart::try_from::<surrealdb::sql::Object> called with an object with a non-Array `id` value"),
        };

        let event = match value.remove("event") {
            Some(surrealdb::sql::Value::Strand(event)) => Some(String::from(event)),
            Some(surrealdb::sql::Value::None) => None,
            None => None,
            _ => bail!("PrincipalChainIdPart::try_from::<surrealdb::sql::Object> called with an object containing an invalid `event` value"),
        };

        ensure!(value.is_empty(), "PrincipalChainIdPart::try_from::<surrealdb::sql::Object> called with an invalid object containing extra keys");

        Ok(PrincipalChainIdPart { id, event })
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PrincipalChainId(Vec<PrincipalChainIdPart>);

impl std::ops::Deref for PrincipalChainId {
    type Target = Vec<PrincipalChainIdPart>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl TryFrom<surrealdb::sql::Array> for PrincipalChainId {
    type Error = anyhow::Error;

    fn try_from(value: surrealdb::sql::Array) -> Result<Self, Self::Error> {
        Ok(PrincipalChainId(
            value.into_iter().map(|part| match part {
                surrealdb::sql::Value::Object(part) => PrincipalChainIdPart::try_from(part),
                _ => bail!("PrincipalChainIdPart::try_from::<surrealdb::sql::Array> called with a non-object PrincipalChainIdPart element"),
            }).collect::<anyhow::Result<_>>()?
      ))
    }
}

impl From<PrincipalChainId> for surrealdb::sql::Array {
    fn from(value: PrincipalChainId) -> Self {
        surrealdb::sql::Array::from(
            value
                .0
                .into_iter()
                .map(surrealdb::sql::Value::from)
                .collect::<Vec<_>>(),
        )
    }
}

impl<'de> Deserialize<'de> for PrincipalChainId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = PrincipalChainId;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a PrincipalChainId")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut parts = Vec::new();

                while let Some(part) = seq.next_element()? {
                    parts.push(part);
                }

                Ok(PrincipalChainId(parts))
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut valid_table = false;
                let mut principal_chain_id = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        "tb" => {
                            let table: String = map.next_value()?;
                            if table != "principal_chain" {
                                return Err(serde::de::Error::invalid_value(
                                    serde::de::Unexpected::Str(&table),
                                    &"A SurrealDB PrincipalChainId must be a map with a 'tb' key with a value of a 'principal_chain'",
                                ));
                            }
                            valid_table = true;
                        }
                        "id" => {
                            let id: surrealdb::sql::Id = map.next_value()?;

                            match id {
                                surrealdb::sql::Id::Array(parts) => {
                                    principal_chain_id =
                                        Some(PrincipalChainId::try_from(parts).map_err(|err| {
                                            serde::de::Error::custom(format!(
                                                "Error parsing PrincipalChainId: {err}"
                                            ))
                                        })?);
                                }
                                _ => {
                                    return Err(serde::de::Error::invalid_value(
                                        serde::de::Unexpected::Other("non-array"),
                                        &"A SurrealDB PrincipalChainId must be a map with an 'id' key with an array value",
                                    ));
                                }
                            }
                        }
                        _ => {
                            return Err(serde::de::Error::unknown_field(key, &["tb", "id"]));
                        }
                    }
                }

                if !valid_table {
                    return Err(serde::de::Error::missing_field("tb"));
                }

                if let Some(id) = principal_chain_id {
                    Ok(id)
                } else {
                    Err(serde::de::Error::missing_field("id"))
                }
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GetRequest {
    id: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct GetResponse {
    first_seen_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
}

pub(super) async fn get(
    Extension(db): Extension<Surreal<Any>>,
    Query(GetRequest { id }): Query<GetRequest>,
) -> crate::Result<Json<GetResponse>> {
    let id: PrincipalChainId = match serde_json::from_str(&id) {
        Ok(id) => id,
        Err(err) => bad_request!("Invalid `id` query parameter: {err}"),
    };

    let res = db
        .query("SELECT first_seen_at, last_seen_at FROM type::thing('principal_chain', $id)")
        .bind(("id", surrealdb::sql::Array::from(id)))
        .await?
        .check_first_real_error()?
        .take(0)?;

    match res {
        Some(res) => Ok(Json(res)),
        None => not_found!("Principal chain does not exist"),
    }
}
