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

use core::clone::Clone;
use std::collections::HashSet;

use krator::ObjectState;
pub use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::postgres::operator::PostgresOperatorState;
use crate::postgres::postgres_status::PostgresStatus;
pub(crate) use create_database::CreateDatabase;
pub(crate) use error::Error;
pub(crate) use released::Released;
pub(crate) use wait_for_changes::WaitForChanges;

use crate::postgres::Postgres;

// use crate::postgres::operator::PostgresOperatorState;
// pub use crate::postgres::postgres_status::PostgresStatus;
// pub use crate::postgres::{postgres::DEFAULT_MANIFESTS_SECRET, Postgres, PostgresSpec};

mod create_database;
mod error;
mod released;
mod transitions;
mod wait_for_changes;

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub enum PostgresPhase {
    Initializing,
    CreatingNamespace,
    SettingUpRBACPermissions,
    ApplyingManifests,
    FailedDueToError,
    WaitingForChanges,
}

pub struct PostgresState {
    pub name: String,
    pub error: String,
    pub applied_one_shot_resources: HashSet<String>,
}

#[async_trait::async_trait]
impl ObjectState for PostgresState {
    type Manifest = Postgres;
    type Status = PostgresStatus;
    type SharedState = PostgresOperatorState;
    async fn async_drop(self, _shared: &mut Self::SharedState) {}
}
