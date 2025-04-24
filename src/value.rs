use std::collections::HashMap;

pub(crate) fn surrealdb_value_from_json_value(value: serde_json::Value) -> surrealdb::sql::Value {
    match value {
        serde_json::Value::Null => surrealdb::sql::Value::Null,
        serde_json::Value::Bool(value) => surrealdb::sql::Value::Bool(value),
        serde_json::Value::Number(value) => {
            let value = if let Some(value) = value.as_i64() {
                surrealdb::sql::Number::from(value)
            } else if let Some(value) = value.as_f64() {
                surrealdb::sql::Number::from(value)
            } else if let Some(value) = value.as_u64() {
                surrealdb::sql::Number::from(value)
            } else {
                unreachable!("Invalid serde_json::Value::Number ({value})");
            };

            surrealdb::sql::Value::Number(value)
        }
        serde_json::Value::String(value) => {
            surrealdb::sql::Value::Strand(surrealdb::sql::Strand::from(value))
        }
        serde_json::Value::Array(vec) => surrealdb::sql::Value::Array(
            vec.into_iter()
                .map(surrealdb_value_from_json_value)
                .collect(),
        ),
        serde_json::Value::Object(map) => surrealdb::sql::Value::Object(
            map.into_iter()
                .map(|(key, value)| (key, surrealdb_value_from_json_value(value)))
                .collect::<HashMap<_, _>>()
                .into(),
        ),
    }
}
