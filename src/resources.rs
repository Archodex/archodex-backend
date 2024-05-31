use std::collections::HashMap;

use axum::Json;
use serde::{Deserialize, Serialize};
use surrealdb::{engine::local::Db, sql::Thing, Surreal};

use crate::{
    db,
    error::Error,
    report::{
        Report,
        Repository::{Parameters, Secrets},
        RepositoryType,
        Resource::Secret,
        ResourceType,
    },
    DEMO_ACCOUNT_ID,
};

#[derive(Debug, Serialize, Deserialize)]
struct Context {
    attributes: HashMap<String, String>,
}

impl From<&HashMap<String, String>> for Context {
    fn from(value: &HashMap<String, String>) -> Self {
        Context {
            attributes: value.to_owned(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Project {
    name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Environment {
    name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Repository {
    context: Thing,
    repository_id: String,
    r#type: RepositoryType,
}

#[derive(Debug, Serialize, Deserialize)]
struct Resource {
    repository: Thing,
    resource_id: String,
    r#type: ResourceType,
    hash_value: Option<Thing>,
}

pub async fn post_report(Json(report): Json<Report>) -> Result<(), Error> {
    let db = db(DEMO_ACCOUNT_ID).await;

    let context = get_or_create(
        db,
        &Context {
            attributes: report.context.to_owned(),
        },
    )
    .await?;

    if let Some(name) = report.context.get("project") {
        let project = get_or_create(
            db,
            &Project {
                name: name.to_owned(),
            },
        )
        .await?;

        context_set_default_project(db, &context, &project).await?;
    }

    if let Some(name) = report.context.get("environment") {
        let environment = get_or_create(
            db,
            &Environment {
                name: name.to_owned(),
            },
        )
        .await?;

        context_set_default_environment(db, &context, &environment).await?;
    }

    for repo in report.repositories {
        let (repo_type, repo_inner) = match repo {
            Secrets(inner) => (RepositoryType::Secrets, inner),
            Parameters(inner) => (RepositoryType::Parameters, inner),
        };

        let repo = get_or_create(
            db,
            &Repository {
                context: context.to_owned(),
                repository_id: repo_inner.id,
                r#type: repo_type,
            },
        )
        .await?;

        for resource in repo_inner.resources {
            match resource {
                Secret(secret) => {
                    let hash = get_or_create_hash_value(db, &secret.hash).await?;

                    get_or_create(
                        db,
                        &Resource {
                            repository: repo.to_owned(),
                            r#type: ResourceType::Secret,
                            resource_id: secret.id,
                            hash_value: Some(hash),
                        },
                    )
                    .await?;
                }
                crate::report::Resource::Parameter(_) => todo!(),
            }
        }
    }

    Ok(())
}

async fn context_set_default_project(
    db: &Surreal<Db>,
    context: &Thing,
    project: &Thing,
) -> Result<(), surrealdb::Error> {
    db.query("RELATE ONLY $context -> context_default_project -> $project")
        .bind(("context", context))
        .bind(("project", project))
        .await?;

    Ok(())
}

async fn context_set_default_environment(
    db: &Surreal<Db>,
    context: &Thing,
    environment: &Thing,
) -> Result<(), surrealdb::Error> {
    db.query("RELATE ONLY $context -> context_default_environment -> $environment")
        .bind(("context", context))
        .bind(("environment", environment))
        .await?;

    Ok(())
}

async fn get_or_create<T: core::fmt::Debug + Serialize>(
    db: &Surreal<Db>,
    record: &T,
) -> Result<Thing, surrealdb::Error> {
    #[derive(Debug, Serialize, Deserialize)]
    struct Record {
        id: Thing,
    }

    let record_type = std::any::type_name::<T>()
        .split("::")
        .last()
        .unwrap()
        .to_lowercase();

    let res: Result<Vec<Record>, _> = db.create(record_type).content(record).await;

    match res {
        Ok(mut values) => Ok(values.remove(0).id),
        Err(err) => match err {
            surrealdb::Error::Db(surrealdb::error::Db::IndexExists { thing, .. }) => Ok(thing),
            _ => Err(err),
        },
    }
}

async fn get_or_create_hash_value(
    db: &Surreal<Db>,
    hash_value: &[u8; 32],
) -> Result<Thing, surrealdb::Error> {
    #[derive(Debug, Serialize, Deserialize)]
    struct Record {
        id: Thing,
    }

    let record: Record = db
        .insert(("hash_value", hex::encode(hash_value)))
        .await?
        .unwrap();

    Ok(record.id)
}
