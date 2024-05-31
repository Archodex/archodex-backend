use core::fmt::Debug;
use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct PrimitiveResource {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    attributes: Option<HashMap<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SecretResource {
    pub(crate) id: String,
    #[serde(with = "hex::serde")]
    pub(crate) hash: [u8; 32],
    #[serde(skip_serializing_if = "Option::is_none")]
    attributes: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) enum ResourceType {
    Secret,
    Parameter,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub(crate) enum Resource {
    Secret(SecretResource),
    Parameter(PrimitiveResource),
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct PrimitiveRepository {
    pub(crate) id: String,
    pub(crate) resources: Vec<Resource>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) enum RepositoryType {
    Secrets,
    Parameters,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum Repository {
    Secrets(PrimitiveRepository),
    Parameters(PrimitiveRepository),
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct Report {
    pub(crate) context: HashMap<String, String>,
    pub(crate) repositories: Vec<Repository>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_context() {
        let obj = Report {
            context: HashMap::from([("foo".to_string(), "bar".to_string())]),
            repositories: vec![],
        };

        let mut json = r#"{
            "context": {
                "foo": "bar"
            },
            "repositories": []
        }"#
        .to_string();

        json.retain(|c| !c.is_whitespace());

        assert_eq!(serde_json::to_string(&obj).unwrap(), json);

        assert_eq!(serde_json::from_str::<Report>(&json).unwrap(), obj);
    }

    #[test]
    fn secrets_repo() {
        let obj = Repository::Secrets(PrimitiveRepository {
            id: "https://vault.demo.servicearch.com/::secret_v1".to_string(),
            resources: vec![Resource::Secret(SecretResource {
                id: "db_creds::$.password".to_string(),
                hash: hex::decode(
                    "b5bb9d8014a0f9b1d61e21e796d78dccdf1352f23cd32812f4850b878ae4944c",
                )
                .unwrap()
                .try_into()
                .unwrap(),
                attributes: None,
                created_at: Some(
                    DateTime::parse_from_rfc3339("2024-05-25T00:53:14Z")
                        .unwrap()
                        .with_timezone(&Utc),
                ),
            })],
        });

        let mut json = r#"{
            "type": "Secrets",
            "id": "https://vault.demo.servicearch.com/::secret_v1",
            "resources": [
                {
                    "type": "Secret",
                    "id": "db_creds::$.password",
                    "hash": "b5bb9d8014a0f9b1d61e21e796d78dccdf1352f23cd32812f4850b878ae4944c",
                    "created_at": "2024-05-25T00:53:14Z"
                }
            ]
        }"#
        .to_string();

        json.retain(|c| !c.is_whitespace());

        assert_eq!(serde_json::to_string(&obj).unwrap(), json);

        assert_eq!(serde_json::from_str::<Repository>(&json).unwrap(), obj);
    }

    /*#[test]
    fn secret() {
        let obj = Resource::Secret(SecretResource {
            id: "db_creds::$.password".to_string(),
            hash: hex::decode("b5bb9d8014a0f9b1d61e21e796d78dccdf1352f23cd32812f4850b878ae4944c")
                .unwrap()
                .try_into()
                .unwrap(),
            attributes: None,
            created_at: Some(
                DateTime::parse_from_rfc3339("2024-05-25T00:53:14Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
        });

        let mut json = r#"{
            "type": "Secret",
            "repository": {
                "type": "Secrets",
                "id": "https://vault.demo.servicearch.com/::secret_v1"
            },
            "id": "db_creds::$.password",
            "hash": "b5bb9d8014a0f9b1d61e21e796d78dccdf1352f23cd32812f4850b878ae4944c",
            "created_at": "2024-05-25T00:53:14Z"
        }"#
        .to_string();

        json.retain(|c| !c.is_whitespace());

        assert_eq!(serde_json::to_string(&obj).unwrap(), json);

        assert_eq!(serde_json::from_str::<Resource>(&json).unwrap(), obj);
    }*/
}
