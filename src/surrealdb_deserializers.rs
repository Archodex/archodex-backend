pub(crate) mod string {
    use serde::Deserialize;

    pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<String, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = String;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a String or a SurrealDB RecordId")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(v.to_string())
            }

            fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(v)
            }

            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let record_id = surrealdb::RecordId::deserialize(
                    serde::de::value::MapAccessDeserializer::new(map),
                )?;
                let key_value = surrealdb::Value::from(record_id.key().clone());
                let key = surrealdb::value::from_value::<String>(key_value).unwrap();
                Ok(key)
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

pub(crate) mod u32 {
    use serde::Deserialize;

    pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<u32, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = u32;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a positive integer or a SurrealDB RecordId")
            }

            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if v < 0 {
                    Err(serde::de::Error::invalid_value(
                        serde::de::Unexpected::Signed(v),
                        &self,
                    ))
                } else {
                    Ok(v as u32)
                }
            }

            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let record_id = surrealdb::RecordId::deserialize(
                    serde::de::value::MapAccessDeserializer::new(map),
                )?;
                let key_value = surrealdb::Value::from(record_id.key().clone());
                let key = surrealdb::value::from_value::<u32>(key_value).unwrap();
                Ok(key)
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

pub(crate) mod uuid {
    use serde::Deserialize;
    use surrealdb::Uuid;

    pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<Uuid, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Uuid;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a String in UUID format or a SurrealDB RecordId")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Uuid::parse_str(v).or_else(|_| {
                    Err(serde::de::Error::invalid_value(
                        serde::de::Unexpected::Str(v),
                        &self,
                    ))
                })
            }

            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let record_id = surrealdb::RecordId::deserialize(
                    serde::de::value::MapAccessDeserializer::new(map),
                )?;
                let key_value = surrealdb::Value::from(record_id.key().clone());
                surrealdb::value::from_value::<Uuid>(key_value).or_else(|_| {
                    Err(serde::de::Error::invalid_value(
                        serde::de::Unexpected::Other("record ID with non-UUID key"),
                        &self,
                    ))
                })
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}
