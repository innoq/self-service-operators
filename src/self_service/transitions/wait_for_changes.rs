use std::sync::Arc;

use krator::{Manifest, State, Transition, TransitionTo};
use kube::api::{ListParams, WatchEvent};
use tokio::sync::RwLock;

use crate::project::Project;
use crate::project::ProjectStatus;
use crate::self_service::transitions::create_namespace::CreateNamespace;
use crate::self_service::transitions::error::Error;
use crate::self_service::transitions::{ProjectPhase, ProjectState, SharedState};
use futures::{StreamExt, TryStreamExt};

#[derive(Debug, Default)]
/// Project is sleeping.
pub(crate) struct WaitForChanges;

impl TransitionTo<WaitForChanges> for WaitForChanges {}

impl TransitionTo<CreateNamespace> for WaitForChanges {}

impl TransitionTo<Error> for WaitForChanges {}

#[async_trait::async_trait]
impl State<ProjectState> for WaitForChanges {
    async fn next(
        self: Box<Self>,
        shared: Arc<RwLock<SharedState>>,
        state: &mut ProjectState,
        manifest: Manifest<Project>,
    ) -> Transition<ProjectState> {
        let lp = &ListParams::default().fields(&format!("metadata.name={}", state.name));
        let mut stream = kube::Api::<Project>::all(shared.read().await.client.clone())
            .watch(lp, &(manifest.latest().metadata.resource_version.unwrap()))
            .await
            .unwrap()
            .boxed();

        match stream.try_next().await {
            Ok(Some(status)) => match status.clone() {
                WatchEvent::Modified(_resource) => {
                    info!("project {} modified", state.name);
                    return Transition::next(self, CreateNamespace);
                }
                WatchEvent::Error(e) => {
                    warn!(
                        "ERROR watching Project with name {}: {}",
                        state.name, e.message
                    );
                }
                _ => debug!(
                    "unimplemented state while watching for changes on Project with name {}: {:?}",
                    state.name, status
                ),
            },
            Err(e) => {
                state.error = e.to_string();
                return Transition::next(self, Error);
            }
            _ => {
                print!("#");
            }
        }

        Transition::next(self, WaitForChanges)
    }

    async fn status(
        &self,
        _state: &mut ProjectState,
        _manifest: &Project,
    ) -> anyhow::Result<ProjectStatus> {
        debug!("status() in WaitForChanges");
        Ok(ProjectStatus {
            phase: Some(ProjectPhase::WaitingForChanges),
            message: Some("waiting for changes".to_string()),
            summary: Some("waiting for changes".to_string()),
        })
    }
}
