use std::sync::Arc;

use k8s_openapi::api::core::v1::Namespace;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};
use krator::{Manifest, State, Transition, TransitionTo};
use kube::api::PostParams;
use tokio::sync::RwLock;

use crate::helper;
use crate::project::Project;
use crate::project::ProjectStatus;
use crate::self_service::transitions::error::Error;
use crate::self_service::transitions::{
    ProjectPhase, ProjectState, SetupRBACPermissions, SharedState,
};

#[derive(Debug, Default)]
/// Project is creating a namespace
pub struct CreateNamespace;

impl TransitionTo<SetupRBACPermissions> for CreateNamespace {}

impl TransitionTo<Error> for CreateNamespace {}

#[async_trait::async_trait]
impl State<ProjectState> for CreateNamespace {
    async fn next(
        self: Box<Self>,
        shared: Arc<RwLock<SharedState>>,
        state: &mut ProjectState,
        manifest: Manifest<Project>,
    ) -> Transition<ProjectState> {
        info!("creating namespace {}", &state.name);

        let api: kube::Api<Namespace> = kube::Api::all(shared.read().await.client.clone());
        let project = manifest.latest();
        let name = project.clone().metadata.name.unwrap();

        if let Ok(namespace) = api.get(&name).await {
            if helper::is_owned_by_project(&project, &namespace) {
                return Transition::next(self, SetupRBACPermissions);
            } else {
                state.error = format!(
                    "namespace '{}' exists but does not belong to project '{}'",
                    &name, &name
                );
                return Transition::next(self, Error);
            }
        }

        let namespace = Namespace {
            metadata: ObjectMeta {
                name: Some(name),
                owner_references: Some(vec![OwnerReference::from(&project)]),
                ..Default::default()
            },
            ..Default::default()
        };

        if let Err(e) = api.create(&PostParams::default(), &namespace).await {
            state.error = format!("error creating namespace {}: {}", state.name, e.to_string());
            Transition::next(self, Error)
        } else {
            Transition::next(self, SetupRBACPermissions)
        }
    }

    async fn status(
        &self,
        state: &mut ProjectState,
        _manifest: &Project,
    ) -> anyhow::Result<ProjectStatus> {
        debug!("status() in CreateNamespace");
        Ok(ProjectStatus {
            phase: Some(ProjectPhase::CreatingNamespace),
            message: Some(format!("creating namespace {}", state.name)),
            summary: Some(format!("creating namespace {}", state.name)),
        })
    }
}
