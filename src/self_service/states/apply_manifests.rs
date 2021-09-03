use std::sync::Arc;

use krator::{Manifest, State, Transition};
use tokio::sync::RwLock;

use crate::self_service::helper;
use crate::self_service::project::Project;
use crate::self_service::project::ProjectStatus;
use crate::self_service::states::{Error, SharedState};
use crate::self_service::states::{ProjectPhase, ProjectState, WaitForChanges};

#[derive(Debug, Default)]
pub(crate) struct ApplyManifests;

#[async_trait::async_trait]
impl State<ProjectState> for ApplyManifests {
    async fn next(
        self: Box<Self>,
        shared: Arc<RwLock<SharedState>>,
        state: &mut ProjectState,
        manifest: Manifest<Project>,
    ) -> Transition<ProjectState> {
        let shared = shared.read().await;
        let project = manifest.latest();
        let delay = shared.manifest_retry_delay;

        let manifests = project
            .associated_manifests(
                &shared.client,
                &shared.default_manifests_secret,
                &shared.default_ns,
            )
            .await;

        match manifests {
            Err(e) => {
                state.error = e.to_string();
                return Transition::next(self, Error);
            }

            Ok(manifests) => {
                let mut manifests = manifests
                    .iter()
                    .map(|s| (0, s))
                    .collect::<Vec<(u16, &String)>>();

                let mut iteration = 0;
                let max_retries = 5;
                while let Some((i, manifest)) = manifests.pop() {
                    if let Err(e) =
                        helper::apply_yaml_manifest(&shared.client, &manifest, &project).await
                    {
                        if i >= max_retries {
                            state.error = format!(
                                "error installing manifest: giving up after {} retries: {}\nmanifest was:\n{}",
                                i,
                                e.to_string(),
                                &manifest
                            );
                            return Transition::next(self, Error);
                        }
                        if i != iteration {
                            iteration = i;
                            tokio::time::sleep(delay).await;
                        }
                        manifests.insert(0, (i + 1, &manifest));
                    }
                }
            }
        }

        Transition::next(self, WaitForChanges)
    }

    async fn status(
        &self,
        _state: &mut ProjectState,
        _manifest: &Project,
    ) -> anyhow::Result<ProjectStatus> {
        debug!("status() in ApplyManifests");
        Ok(ProjectStatus {
            phase: Some(ProjectPhase::ApplyingManifests),
            message: Some("applying configured manifests".to_string()),
            summary: Some("applying configured manifests".to_string()),
        })
    }
}
