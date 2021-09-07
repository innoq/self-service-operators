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
use std::time::Duration;

use krator::ObjectState;
pub use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub(crate) use apply_manifests::ApplyManifests;
pub(crate) use create_namespace::CreateNamespace;
pub(crate) use error::Error;
pub(crate) use released::Released;
pub(crate) use wait_for_changes::WaitForChanges;

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
    pub(crate) name: String,
    pub(crate) _spec: ProjectSpec,
    pub(crate) error: String,
}

#[async_trait::async_trait]
impl ObjectState for ProjectState {
    type Manifest = Project;
    type Status = ProjectStatus;
    type SharedState = SharedState;
    async fn async_drop(self, _shared: &mut Self::SharedState) {}
}

pub struct SharedState {
    pub(crate) client: kube::Client,
    pub(crate) default_manifests_secret: String,
    pub(crate) default_ns: String,
    pub(crate) manifest_retry_delay: Duration,
}

impl Default for SharedState {
    fn default() -> Self {
        SharedState {
            default_manifests_secret: DEFAULT_MANIFESTS_SECRET.to_string(),
            manifest_retry_delay: Duration::from_secs(5),
            ..Default::default()
        }
    }
}

impl SharedState {
    pub fn client(&self) -> kube::Client {
        self.client.clone()
    }
}
