use std::collections::HashSet;

use axum::{Extension, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use surrealdb::{engine::any::Any, Surreal};

use archodex_error::{anyhow, bail, ensure};

#[derive(Clone, Debug, Eq, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResourceIdPart {
    pub(crate) r#type: String,
    pub(crate) id: String,
}

impl From<ResourceIdPart> for surrealdb::sql::Value {
    fn from(value: ResourceIdPart) -> Self {
        surrealdb::sql::Array::from(vec![value.r#type, value.id]).into()
    }
}

impl TryFrom<surrealdb::sql::Array> for ResourceIdPart {
    type Error = anyhow::Error;

    fn try_from(mut value: surrealdb::sql::Array) -> Result<Self, Self::Error> {
        ensure!(value.len() == 2, "ResourceIdPart::from(surrealdb::sql::Array) called with an array with a length other than two");

        let id = if let surrealdb::sql::Value::Strand(id) = value.pop().unwrap() {
            id.into()
        } else {
            bail!("ResourceIdPart::from(surrealdb::sql::Array) called with an array with a non-strand second element");
        };

        let r#type = if let surrealdb::sql::Value::Strand(r#type) = value.pop().unwrap() {
            r#type.into()
        } else {
            bail!("ResourceIdPart::from(surrealdb::sql::Array) called with an array with a non-strand first element");
        };

        Ok(ResourceIdPart { r#type, id })
    }
}

impl<'de> Deserialize<'de> for ResourceIdPart {
    fn deserialize<D>(deserializer: D) -> Result<ResourceIdPart, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = ResourceIdPart;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a resource ID part")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut r#type = None;
                let mut id = None;

                while let Some(key) = map.next_key::<&str>()? {
                    match key {
                        "type" => {
                            if r#type.is_some() {
                                return Err(serde::de::Error::duplicate_field("type"));
                            }
                            r#type = Some(map.next_value()?);
                        }
                        "id" => {
                            if id.is_some() {
                                return Err(serde::de::Error::duplicate_field("id"));
                            }
                            id = Some(map.next_value()?);
                        }
                        _ => {
                            return Err(serde::de::Error::unknown_field(key, &["type", "id"]));
                        }
                    }
                }

                let r#type = r#type.ok_or_else(|| serde::de::Error::missing_field("type"))?;
                let id = id.ok_or_else(|| serde::de::Error::missing_field("id"))?;

                Ok(ResourceIdPart { r#type, id })
            }

            fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut seq = seq;

                let r#type = seq.next_element()?.ok_or_else(|| {
                    serde::de::Error::invalid_length(
                        0,
                        &"A ResourceIdPart must have two string elements",
                    )
                })?;
                let id = seq.next_element()?.ok_or_else(|| {
                    serde::de::Error::invalid_length(
                        1,
                        &"A ResourceIdPart must have two string elements",
                    )
                })?;

                if seq.next_element::<String>()?.is_some() {
                    return Err(serde::de::Error::invalid_length(
                        3,
                        &"A ResourceId must have two string elements",
                    ));
                }

                Ok(ResourceIdPart { r#type, id })
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

#[derive(Clone, Debug, Eq, Serialize, PartialEq)]
pub(crate) struct ResourceId(Vec<ResourceIdPart>);

impl std::ops::Deref for ResourceId {
    type Target = Vec<ResourceIdPart>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<ResourceId> for surrealdb::sql::Array {
    fn from(value: ResourceId) -> Self {
        surrealdb::sql::Array::from(
            value
                .into_iter()
                .map(surrealdb::sql::Value::from)
                .collect::<Vec<_>>(),
        )
    }
}

impl From<ResourceId> for surrealdb::sql::Value {
    fn from(value: ResourceId) -> Self {
        surrealdb::sql::Array::from(value).into()
    }
}

pub(crate) fn surrealdb_thing_from_resource_id(value: ResourceId) -> surrealdb::sql::Value {
    surrealdb::sql::Thing::from((
        "resource",
        surrealdb::sql::Id::from(surrealdb::sql::Array::from(value)),
    ))
    .into()
}

impl TryFrom<surrealdb::sql::Array> for ResourceId {
    type Error = anyhow::Error;

    fn try_from(value: surrealdb::sql::Array) -> Result<Self, Self::Error> {
        Ok(ResourceId(
            value.into_iter().map(|part| match part {
                surrealdb::sql::Value::Array(part) => ResourceIdPart::try_from(part),
                _ => bail!("ResourceId::try_from::<surrealdb::sql::Array> called with a non-array ResourceIdPart element"),
            }).collect::<anyhow::Result<_>>()?
        ))
    }
}

impl IntoIterator for ResourceId {
    type Item = ResourceIdPart;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'de> Deserialize<'de> for ResourceId {
    fn deserialize<D>(deserializer: D) -> Result<ResourceId, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = ResourceId;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a resource ID")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut parts = Vec::new();

                while let Some(part) = seq.next_element()? {
                    parts.push(part);
                }

                Ok(ResourceId(parts))
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut valid_table = false;
                let mut resource_id = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        "tb" => {
                            let table: String = map.next_value()?;
                            if table != "resource" {
                                return Err(serde::de::Error::invalid_value(
                                    serde::de::Unexpected::Str(&table),
                                    &"A SurrealDB ResourceId must be a map with a 'tb' key with a value of a 'resource'",
                                ));
                            }
                            valid_table = true;
                        }
                        "id" => {
                            let id: surrealdb::sql::Id = map.next_value()?;

                            match id {
                                surrealdb::sql::Id::Array(parts) => {
                                    resource_id =
                                        Some(ResourceId::try_from(parts).map_err(|err| {
                                            serde::de::Error::custom(format!(
                                                "Error parsing ResourceId: {err}"
                                            ))
                                        })?);
                                }
                                _ => {
                                    return Err(serde::de::Error::invalid_value(
                                        serde::de::Unexpected::Other("non-array"),
                                        &"A SurrealDB ResourceId must be a map with an 'id' key with an array value",
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

                if let Some(id) = resource_id {
                    Ok(id)
                } else {
                    Err(serde::de::Error::missing_field("id"))
                }
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Resource {
    pub(crate) id: ResourceId,
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub(crate) environments: HashSet<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) first_seen_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_seen_at: Option<DateTime<Utc>>,
}

impl Resource {
    pub(crate) fn get_all() -> &'static str {
        "$resources = SELECT * FROM resource WHERE id != resource:[] PARALLEL;"
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SetTagsRequest {
    resource_id: ResourceId,
    environments: HashSet<String>,
}

pub(super) async fn set_environments(
    Extension(db): Extension<Surreal<Any>>,
    Json(req): Json<SetTagsRequest>,
) -> crate::Result<()> {
    const QUERY: &str =
        "BEGIN; UPDATE resource SET environments = $envs WHERE id = $resource_id; COMMIT;";

    db.query(QUERY)
        .bind(("envs", req.environments))
        .bind((
            "resource_id",
            surrealdb_thing_from_resource_id(req.resource_id),
        ))
        .await?;

    Ok(())
}
