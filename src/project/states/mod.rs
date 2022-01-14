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

pub(crate) use apply_manifests::ApplyManifests;
pub(crate) use create_namespace::CreateNamespace;
pub(crate) use error::Error;
pub(crate) use released::Released;
pub(crate) use wait_for_changes::WaitForChanges;

use crate::project::operator::ProjectOperatorState;
pub use crate::project::project_status::ProjectStatus;
pub use crate::project::{project::DEFAULT_MANIFESTS_SECRET, Project, ProjectSpec};

pub mod apply_manifests;
mod create_namespace;
mod error;
mod released;
mod transitions;
mod wait_for_changes;

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub enum ProjectPhase {
    Initializing,
    CreatingNamespace,
    SettingUpRBACPermissions,
    ApplyingManifests,
    FailedDueToError,
    WaitingForChanges,
}

pub struct ProjectState {
    pub name: String,
    pub error: String,
    pub applied_one_shot_resources: HashSet<String>,
}

#[async_trait::async_trait]
impl ObjectState for ProjectState {
    type Manifest = Project;
    type Status = ProjectStatus;
    type SharedState = ProjectOperatorState;
    async fn async_drop(self, _shared: &mut Self::SharedState) {}
}
