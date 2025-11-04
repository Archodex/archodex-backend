pub(crate) mod string {
    use serde::Deserialize;
    use tracing::instrument;

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

            #[instrument(err, skip_all)]
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
    use tracing::instrument;

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

            #[instrument(err, skip(self))]
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
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    Ok(v as Self::Value)
                }
            }

            #[instrument(err, skip_all)]
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
    use std::marker::PhantomData;

    use serde::{Deserialize, de::VariantAccess};
    use surrealdb::Uuid;
    use tracing::instrument;

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

            #[instrument(err, skip_all)]
            fn visit_enum<A>(self, data: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::EnumAccess<'de>,
            {
                let (variant, value) = data
                    .variant::<String>()
                    .map_err(|err| {
                        serde::de::Error::custom(format!("Failed to deserialize surrealdb::sql::Uuid: Enum variant type could not be deserialized as a `String`: {err}"))
                    })?;

                if variant != "Uuid" {
                    return Err(serde::de::Error::unknown_variant(&variant, &["Uuid"]));
                }

                let sql_uuid = value
                    .newtype_variant_seed::<PhantomData<surrealdb::sql::Uuid>>(PhantomData)
                    .map_err(|_| {
                        serde::de::Error::invalid_value(
                            serde::de::Unexpected::NewtypeVariant,
                            &"a surrealdb::sql::Uuid",
                        )
                    })?;

                Ok(sql_uuid.into())
            }

            #[instrument(err, skip(self))]
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Uuid::parse_str(v).map_err(|_| {
                    serde::de::Error::invalid_value(serde::de::Unexpected::Str(v), &self)
                })
            }

            #[instrument(err, skip_all)]
            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let record_id = surrealdb::RecordId::deserialize(
                    serde::de::value::MapAccessDeserializer::new(map),
                )?;
                let key_value = surrealdb::Value::from(record_id.key().clone());
                surrealdb::value::from_value::<Uuid>(key_value).map_err(|_| {
                    serde::de::Error::invalid_value(
                        serde::de::Unexpected::Other("record ID with non-UUID key"),
                        &self,
                    )
                })
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

pub(crate) mod bytes {
    struct Visitor;
    type Value = Vec<u8>;

    impl serde::de::Visitor<'_> for Visitor {
        type Value = Value;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a byte array")
        }

        fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(v.to_vec())
        }

        fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(v)
        }
    }

    pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(Visitor)
    }

    #[cfg(not(feature = "archodex-com"))]
    pub(crate) fn deserialize_optional<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct OptionalVisitor;

        impl<'de> serde::de::Visitor<'de> for OptionalVisitor {
            type Value = Option<Value>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("an optional byte array")
            }

            fn visit_none<E>(self) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(None)
            }

            fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                Ok(Some(deserializer.deserialize_any(Visitor)?))
            }
        }

        deserializer.deserialize_option(OptionalVisitor)
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    // Tests for string deserializer
    #[derive(Debug, Deserialize, PartialEq)]
    struct TestString {
        #[serde(deserialize_with = "super::string::deserialize")]
        value: String,
    }

    #[test]
    fn test_string_deserialize_direct_string() {
        let json = r#"{"value": "test-string"}"#;
        let result: TestString = serde_json::from_str(json).unwrap();
        assert_eq!(result.value, "test-string");
    }

    // Tests for u32 deserializer
    /// Validates that the u32 deserializer correctly rejects negative numbers
    #[test]
    fn test_u32_reject_negative_number() {
        use super::u32;
        use serde::de::value::{Error as ValueError, I64Deserializer};

        let deserializer = I64Deserializer::<ValueError>::new(-5);
        let result = u32::deserialize(deserializer);
        assert!(result.is_err());
    }

    /// Validates that the u32 deserializer accepts zero
    #[test]
    fn test_u32_deserialize_zero() {
        use super::u32;
        use serde::de::value::{Error as ValueError, I64Deserializer};

        let deserializer = I64Deserializer::<ValueError>::new(0);
        let result = u32::deserialize(deserializer);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    /// Validates that the u32 deserializer accepts positive numbers
    #[test]
    fn test_u32_deserialize_positive_number() {
        use super::u32;
        use serde::de::value::{Error as ValueError, I64Deserializer};

        let deserializer = I64Deserializer::<ValueError>::new(42);
        let result = u32::deserialize(deserializer);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    // Tests for uuid deserializer
    #[derive(Debug, Deserialize, PartialEq)]
    struct TestUuid {
        #[serde(deserialize_with = "super::uuid::deserialize")]
        value: surrealdb::Uuid,
    }

    #[test]
    fn test_uuid_deserialize_from_string() {
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let json = format!(r#"{{"value": "{uuid_str}"}}"#);
        let result: TestUuid = serde_json::from_str(&json).unwrap();
        assert_eq!(result.value.to_string(), uuid_str);
    }

    #[test]
    fn test_uuid_deserialize_invalid_string() {
        let json = r#"{"value": "not-a-valid-uuid"}"#;
        let result: Result<TestUuid, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_uuid_deserialize_empty_string() {
        let json = r#"{"value": ""}"#;
        let result: Result<TestUuid, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }
}
