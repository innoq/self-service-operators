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

use std::sync::Arc;

use crate::project::operator::ProjectOperatorState;
use futures::{StreamExt, TryStreamExt};
use krator::{Manifest, State, Transition};
use kube::api::{ListParams, WatchEvent};
use tokio::sync::RwLock;

use crate::project::project_status::ProjectStatus;
use crate::project::states::create_namespace::CreateNamespace;
use crate::project::states::error::Error;
use crate::project::states::{ProjectPhase, ProjectState};
use crate::project::Project;

#[derive(Debug, Default)]
/// Project is sleeping.
pub(crate) struct WaitForChanges;

#[async_trait::async_trait]
impl State<ProjectState> for WaitForChanges {
    async fn next(
        self: Box<Self>,
        shared: Arc<RwLock<ProjectOperatorState>>,
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
        project: &Project,
    ) -> anyhow::Result<ProjectStatus> {
        debug!("status() in WaitForChanges");
        Ok(ProjectStatus {
            phase: Some(ProjectPhase::WaitingForChanges),
            message: Some("waiting for changes".to_string()),
            summary: Some("waiting for changes".to_string()),
            applied_one_shot_resources: project
                .status
                .clone().unwrap_or_default()
                .applied_one_shot_resources,
        })
    }
}
