use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{principal_chain::PrincipalChainId, resource::ResourceId};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Event {
    pub(crate) principal: ResourceId,
    pub(crate) r#type: String,
    pub(crate) resource: ResourceId,
    pub(crate) principal_chains: Vec<PrincipalChainId>,
    pub(crate) first_seen_at: DateTime<Utc>,
    pub(crate) last_seen_at: DateTime<Utc>,
}

impl<'de> Deserialize<'de> for Event {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Event;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("an event or a database event record")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut principal: Option<ResourceId> = None;
                let mut r#type: Option<String> = None;
                let mut resource: Option<ResourceId> = None;
                let mut principal_chains: Option<Vec<PrincipalChainId>> = None;
                let mut first_seen_at: Option<DateTime<Utc>> = None;
                let mut last_seen_at: Option<DateTime<Utc>> = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "id" => {
                            map.next_value::<serde::de::IgnoredAny>()?;
                        }
                        "in" if principal.is_none() => principal = Some(map.next_value()?),
                        "in" => Err(serde::de::Error::duplicate_field("in"))?,
                        "principal" if principal.is_none() => principal = Some(map.next_value()?),
                        "principal" => Err(serde::de::Error::duplicate_field("principal"))?,
                        "type" => r#type = Some(map.next_value()?),
                        "out" if resource.is_none() => resource = Some(map.next_value()?),
                        "out" => Err(serde::de::Error::duplicate_field("out"))?,
                        "resource" if resource.is_none() => resource = Some(map.next_value()?),
                        "resource" => Err(serde::de::Error::duplicate_field("resource"))?,
                        "principal_chains" if principal_chains.is_none() => {
                            principal_chains = Some(map.next_value()?)
                        }
                        "principal_chains" => {
                            Err(serde::de::Error::duplicate_field("principal_chains"))?
                        }
                        "has_direct_principal_chain" => {
                            map.next_value::<serde::de::IgnoredAny>()?;
                        }
                        "first_seen_at" => first_seen_at = Some(map.next_value()?),
                        "last_seen_at" => last_seen_at = Some(map.next_value()?),
                        _ => {
                            return Err(serde::de::Error::unknown_field(
                                &key,
                                &[
                                    "id",
                                    "in",
                                    "principal",
                                    "type",
                                    "out",
                                    "resource",
                                    "principal_chains",
                                    "has_direct_principal_chain",
                                    "first_seen_at",
                                    "last_seen_at",
                                ],
                            ));
                        }
                    }
                }

                Ok(Self::Value {
                    principal: principal.ok_or_else(|| serde::de::Error::missing_field("in"))?,
                    r#type: r#type.ok_or_else(|| serde::de::Error::missing_field("type"))?,
                    resource: resource.ok_or_else(|| serde::de::Error::missing_field("out"))?,
                    principal_chains: principal_chains
                        .ok_or_else(|| serde::de::Error::missing_field("principal"))?,
                    first_seen_at: first_seen_at
                        .ok_or_else(|| serde::de::Error::missing_field("first_seen_at"))?,
                    last_seen_at: last_seen_at
                        .ok_or_else(|| serde::de::Error::missing_field("last_seen_at"))?,
                })
            }
        }

        deserializer.deserialize_map(Visitor)
    }
}

impl Event {
    pub(crate) fn get_all() -> &'static str {
        "$events = SELECT * OMIT id FROM event PARALLEL;"
    }
}
