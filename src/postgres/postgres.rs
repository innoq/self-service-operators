/*
 * Copyright 2021 Daniel Bornkessel
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use std::cmp::PartialEq;

use crate::postgres::PostgresStatus;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use krator_derive::AdmissionWebhook;
use kube::CustomResource;
pub use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub trait Sample {
    fn sample() -> Self;
}

#[derive(
    AdmissionWebhook,
    CustomResource,
    Serialize,
    Deserialize,
    PartialEq,
    Default,
    Debug,
    Clone,
    JsonSchema,
)]

/// a self service postgres that will create a database and create a user
#[admission_webhook_features(secret, service, admission_webhook_config)]
#[serde(rename_all = "camelCase")]
#[kube(
    group = "selfservice.innoq.io",
    version = "v1",
    kind = "Postgres",
    plural = "postgres",
    status = "PostgresStatus",
    shortname = "sspsql",
    printcolumn = r#"
     {"name":"Deletion Protection", "type":"boolean", "description":"set to false to make the database deletable", "jsonPath":".spec.deletionProtection"},
     {"name":"Age", "type":"date", "description":"how old this resource is", "jsonPath":".metadata.creationTimestamp"},
     {"name":"Status summary", "type":"string", "description":"current status", "jsonPath":".status.summary"}
  "#
)]
pub struct PostgresSpec {
    /// set to false to make the database deletable
    pub deletion_protection: bool,
}

impl Sample for PostgresSpec {
    fn sample() -> Self {
        PostgresSpec {
            deletion_protection: true,
        }
    }
}

impl Postgres {}

impl From<&Postgres> for OwnerReference {
    fn from(p: &Postgres) -> OwnerReference {
        OwnerReference {
            api_version: p.api_version.clone(),
            block_owner_deletion: None,
            controller: Some(true),
            kind: p.kind.clone(),
            name: p.metadata.name.clone().unwrap(),
            uid: p.metadata.uid.clone().unwrap(),
        }
    }
}

impl Default for Postgres {
    fn default() -> Self {
        Postgres::new("", PostgresSpec::default())
    }
}

impl Sample for Postgres {
    fn sample() -> Self {
        
        Postgres::new("sample-self-service-postgres", PostgresSpec::sample())
    }
}

impl PartialEq for Postgres {
    fn eq(&self, other: &Postgres) -> bool {
        self.metadata.name == other.metadata.name && self.spec == other.spec
    }
}
