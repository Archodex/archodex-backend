use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::{next_binding, surrealdb_deserializers, user::User};

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ReportKey {
    #[serde(deserialize_with = "surrealdb_deserializers::u32::deserialize")]
    id: u32,
    description: Option<String>,
    created_at: Option<DateTime<Utc>>,
    created_by: User,
    #[allow(dead_code)]
    revoked_at: Option<DateTime<Utc>>,
    #[allow(dead_code)]
    revoked_by: Option<User>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ReportKeyPublic {
    #[serde(deserialize_with = "surrealdb_deserializers::u32::deserialize")]
    pub(crate) id: u32,
    pub(crate) description: Option<String>,
    pub(crate) created_at: Option<DateTime<Utc>>,
    pub(crate) created_by: User,
}

impl From<ReportKey> for ReportKeyPublic {
    fn from(record: ReportKey) -> Self {
        Self {
            id: record.id,
            description: record.description,
            created_at: record.created_at,
            created_by: record.created_by,
        }
    }
}

impl ReportKey {
    pub(crate) fn new(description: Option<String>, created_by: User) -> Self {
        Self {
            id: rand::thread_rng().gen_range::<u32, _>(1..=u32::MAX),
            description,
            created_at: None,
            created_by,
            revoked_at: None,
            revoked_by: None,
        }
    }
}

pub(crate) trait ReportKeyQueries<'r, C: surrealdb::Connection> {
    fn list_report_keys_query(self) -> surrealdb::method::Query<'r, C>;
    fn create_report_key_query(self, report_key: &ReportKey) -> surrealdb::method::Query<'r, C>;
    fn revoke_report_key_query(
        self,
        report_key_id: u32,
        revoked_by: &User,
    ) -> surrealdb::method::Query<'r, C>;
    fn report_key_is_valid_query(self, id: u32) -> surrealdb::method::Query<'r, C>;
}

impl<'r, C: surrealdb::Connection> ReportKeyQueries<'r, C> for surrealdb::method::Query<'r, C> {
    fn list_report_keys_query(self) -> surrealdb::method::Query<'r, C> {
        self.query("SELECT * FROM report_key WHERE type::is::none(revoked_at)")
    }

    fn create_report_key_query(self, report_key: &ReportKey) -> surrealdb::method::Query<'r, C> {
        let report_key_binding = next_binding();
        let description_binding = next_binding();
        let created_by_binding = next_binding();

        self
            .query("CREATE $report_key CONTENT {{ description: $description, created_by: $created_by }} RETURN NONE")
            .bind((report_key_binding, surrealdb::sql::Thing::from(report_key)))
            .bind((description_binding, report_key.description.to_owned()))
            .bind((created_by_binding, surrealdb::sql::Thing::from(&report_key.created_by)))
    }

    fn revoke_report_key_query(
        self,
        report_key_id: u32,
        revoked_by: &User,
    ) -> surrealdb::method::Query<'r, C> {
        let report_key_binding = next_binding();
        let revoked_by_binding = next_binding();

        self.query(
            "UPDATE $report_key SET revoked_at = time::now(), revoked_by = $revoked_by WHERE revoked_at IS NONE",
        )
        .bind((
            report_key_binding,
            surrealdb::sql::Thing::from((
                "report_key",
                surrealdb::sql::Id::from(report_key_id as i64),
            )),
        ))
        .bind((revoked_by_binding, surrealdb::sql::Thing::from(revoked_by)))
    }

    fn report_key_is_valid_query(self, report_key_id: u32) -> surrealdb::method::Query<'r, C> {
        let report_key_binding = next_binding();

        self.query("SELECT type::is::none(revoked_at) AS valid FROM $report_key")
            .bind((
                report_key_binding,
                surrealdb::sql::Thing::from((
                    "report_key",
                    surrealdb::sql::Id::from(report_key_id as i64),
                )),
            ))
    }
}

impl From<&ReportKey> for surrealdb::sql::Thing {
    fn from(report_key: &ReportKey) -> Self {
        Self::from((
            "report_key",
            surrealdb::sql::Id::Number(report_key.id as i64),
        ))
    }
}
