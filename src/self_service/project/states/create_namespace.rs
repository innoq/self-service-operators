use std::sync::Arc;

use k8s_openapi::api::core::v1::Namespace;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};
use krator::{Manifest, State, Transition};
use kube::api::PostParams;
use tokio::sync::RwLock;

use crate::self_service::project::states::error::Error;
use crate::self_service::project::states::{
    ApplyManifests, ProjectPhase, ProjectState, SharedState,
};
use crate::self_service::project::Project;
use crate::self_service::project::ProjectStatus;

#[derive(Debug, Default)]
/// Project is creating a namespace
pub struct CreateNamespace;

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
            if is_owned_by_project(&project, &namespace) {
                return Transition::next(self, ApplyManifests);
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
            Transition::next(self, ApplyManifests)
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

pub fn is_owned_by_project<R>(project: &Project, resource: &R) -> bool
where
    R: kube::Resource + k8s_openapi::Resource,
{
    if resource.meta().owner_references.is_none() {
        return false;
    }

    if let Some(owners) = resource.meta().owner_references.as_ref() {
        if owners.is_empty() {
            return false;
        }

        let owner = &owners[0];

        return owner.api_version == project.api_version
            && owner.controller == Some(true)
            && owner.kind == project.kind
            && owner.name == *project.metadata.name.as_ref().unwrap()
            && owner.uid == project.metadata.uid.clone().unwrap_or_else(String::new);
    }

    true
}
