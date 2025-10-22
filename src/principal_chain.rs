use std::collections::HashMap;

use axum::{Extension, Json, extract::Query};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use archodex_error::{anyhow, bad_request, bail, ensure, not_found};
use tracing::instrument;

use crate::{account::AuthedAccount, db::QueryCheckFirstRealError, resource::ResourceId};

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

    #[instrument(err)]
    fn try_from(mut value: surrealdb::sql::Object) -> Result<Self, Self::Error> {
        let Some(id) = value.remove("id") else {
            bail!(
                "PrincipalChainIdPart::try_from::<surrealdb::sql::Object> called with an object missing the `id` key"
            )
        };

        let id = match id {
            surrealdb::sql::Value::Array(id) => ResourceId::try_from(id)?,
            _ => bail!(
                "PrincipalChainIdPart::try_from::<surrealdb::sql::Object> called with an object with a non-Array `id` value"
            ),
        };

        let event = match value.remove("event") {
            Some(surrealdb::sql::Value::Strand(event)) => Some(String::from(event)),
            Some(surrealdb::sql::Value::None) | None => None,
            _ => bail!(
                "PrincipalChainIdPart::try_from::<surrealdb::sql::Object> called with an object containing an invalid `event` value"
            ),
        };

        ensure!(
            value.is_empty(),
            "PrincipalChainIdPart::try_from::<surrealdb::sql::Object> called with an invalid object containing extra keys"
        );

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

    #[instrument(err)]
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

            #[instrument(err, skip_all)]
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

            #[instrument(err, skip_all)]
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

#[instrument(err, skip(authed))]
pub(super) async fn get(
    Extension(authed): Extension<AuthedAccount>,
    Query(GetRequest { id }): Query<GetRequest>,
) -> crate::Result<Json<GetResponse>> {
    let id: PrincipalChainId = match serde_json::from_str(&id) {
        Ok(id) => id,
        Err(err) => bad_request!("Invalid `id` query parameter: {err}"),
    };

    let res = authed
        .resources_db
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::ResourceId;
    use surrealdb::sql::{Object, Strand, Value};

    #[test]
    fn test_principal_chain_id_part_round_trip() {
        // Create test ResourceId using the test helper
        let resource_id =
            ResourceId::from_parts(vec![("partition", "aws"), ("account", "123456789012")]);

        // Create PrincipalChainIdPart with event
        let original = PrincipalChainIdPart {
            id: resource_id.clone(),
            event: Some("s3:PutObject".to_string()),
        };

        // Convert to SurrealDB Value
        let surreal_value: Value = original.clone().into();

        let Value::Object(surreal_object) = surreal_value else {
            panic!("Expected Object, got {surreal_value:?}")
        };

        // Convert back to PrincipalChainIdPart
        let parsed = PrincipalChainIdPart::try_from(surreal_object).unwrap();

        // Verify round-trip correctness
        assert_eq!(parsed.id, original.id);
        assert_eq!(parsed.event, original.event);
    }

    #[test]
    fn test_principal_chain_id_part_without_event() {
        // Create test ResourceId using the test helper
        let resource_id = ResourceId::from_parts(vec![("partition", "aws")]);

        // Create PrincipalChainIdPart without event
        let original = PrincipalChainIdPart {
            id: resource_id.clone(),
            event: None,
        };

        // Convert to SurrealDB Value
        let surreal_value: Value = original.clone().into();
        let Value::Object(surreal_object) = surreal_value else {
            panic!("Expected Object, got {surreal_value:?}")
        };

        // Convert back
        let parsed = PrincipalChainIdPart::try_from(surreal_object).unwrap();

        // Verify round-trip correctness (event should be None)
        assert_eq!(parsed.id, original.id);
        assert_eq!(parsed.event, None);
    }

    #[test]
    fn test_principal_chain_id_part_invalid_object_missing_id() {
        // Create object missing the `id` key
        let mut obj = Object::default();
        obj.insert("event".to_string(), Value::Strand(Strand::from("test")));

        // Attempt to convert should fail
        let result = PrincipalChainIdPart::try_from(obj);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("missing the `id` key")
        );
    }

    #[test]
    fn test_principal_chain_id_part_invalid_event_type() {
        // Create test ResourceId using the test helper
        let resource_id = ResourceId::from_parts(vec![("partition", "aws")]);

        // Create object with invalid event value (number instead of string)
        let mut obj = Object::default();
        obj.insert("id".to_string(), Value::from(resource_id));
        obj.insert("event".to_string(), Value::from(123)); // Invalid: should be string

        // Attempt to convert should fail
        let result = PrincipalChainIdPart::try_from(obj);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("invalid `event` value")
        );
    }
}
