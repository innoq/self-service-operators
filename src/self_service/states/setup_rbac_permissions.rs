use std::sync::Arc;

use k8s_openapi::api::core::v1::Namespace;
use krator::{Manifest, State, Transition};
use kube::Resource;
use tokio::sync::RwLock;

use crate::self_service::helper;
use crate::self_service::project::Project;
use crate::self_service::project::ProjectStatus;
use crate::self_service::states::error::Error;
use crate::self_service::states::{ApplyManifests, ProjectPhase, ProjectState, SharedState};

#[derive(Debug, Default)]
/// Project is sleeping.
pub struct SetupRBACPermissions;

#[async_trait::async_trait]
impl State<ProjectState> for SetupRBACPermissions {
    async fn next(
        self: Box<Self>,
        shared: Arc<RwLock<SharedState>>,
        state: &mut ProjectState,
        manifest: Manifest<Project>,
    ) -> Transition<ProjectState> {
        let project = manifest.latest();

        let (rolebinding, owner_cluster_role, owner_cluster_role_binding) =
            project.rbac_manifests(&shared.read().await.default_owner_cluster_role);
        let client = shared.read().await.client.clone();

        let _ = kube::Api::<Namespace>::all(client.clone())
            .get(project.meta().name.as_ref().unwrap())
            .await;

        for (name, manifest) in vec![
            (
                "clusterrole",
                serde_yaml::to_string(&owner_cluster_role).unwrap(),
            ),
            (
                "clusterrolebinding",
                serde_yaml::to_string(&owner_cluster_role_binding).unwrap(),
            ),
            ("rolebinding", serde_yaml::to_string(&rolebinding).unwrap()),
        ]
        .iter()
        {
            if let Err(e) = helper::apply_yaml_manifest(&client, &manifest, &project).await {
                state.error = format!("error applying {}: {}", name, e);

                return Transition::next(self, Error);
            }
        }

        Transition::next(self, ApplyManifests)
    }

    async fn status(
        &self,
        _state: &mut ProjectState,
        _manifest: &Project,
    ) -> anyhow::Result<ProjectStatus> {
        debug!("status() in SetupRBACPermissions");
        Ok(ProjectStatus {
            phase: Some(ProjectPhase::SettingUpRBACPermissions),
            message: Some("setting up permissions".to_string()),
            summary: Some("setting up permissions".to_string()),
        })
    }
}
