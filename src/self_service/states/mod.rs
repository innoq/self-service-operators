use core::clone::Clone;
use std::time::Duration;

use krator::ObjectState;
pub use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub(crate) use apply_manifests::ApplyManifests;
pub(crate) use create_namespace::CreateNamespace;
pub(crate) use error::Error;
pub(crate) use released::Released;
pub(crate) use setup_rbac_permissions::SetupRBACPermissions;
pub(crate) use wait_for_changes::WaitForChanges;

use crate::self_service::project::{Project, ProjectSpec, ProjectStatus, DEFAULT_MANIFESTS_SECRET};

pub mod apply_manifests;
mod create_namespace;
mod error;
mod released;
mod setup_rbac_permissions;
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
    pub(crate) default_owner_cluster_role: String,
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
