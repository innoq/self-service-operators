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

use std::path::Path;
use std::sync::Arc;

use anyhow::bail;
use anyhow::ensure;
use anyhow::Context;
use http::Request;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};
use krator::{Manifest, State, Transition};
use serde::Deserialize;
use tokio::sync::RwLock;

use crate::project::project_status::ProjectStatus;
use crate::project::states::{Error, SharedState};
use crate::project::states::{ProjectPhase, ProjectState, WaitForChanges};
use crate::project::Project;
use std::ops::Mul;

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

                let mut iteration_counter = 0;
                let max_retries = 5;
                while let Some((i, manifest)) = manifests.pop() {
                    if let Err(e) = apply_yaml_manifest(&shared.client, &manifest, &project).await {
                        if i >= max_retries {
                            state.error = format!(
                                "error installing manifest: giving up after {} retries: {}\nmanifest was:\n{}",
                                i,
                                e.to_string(),
                                &manifest
                            );
                            return Transition::next(self, Error);
                        }

                        // pause between each iteration with backoff factor, so resources that were
                        // applied can get available -- this might be necessary if some resources
                        // depend on others
                        if i != iteration_counter {
                            iteration_counter = i;
                            tokio::time::sleep(delay.mul(i.into())).await;
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

pub async fn apply_yaml_manifest(
    client: &kube::Client,
    yaml_manifest: &str,
    project: &Project,
) -> anyhow::Result<()> {
    let path = resource_path(&client, yaml_manifest).await?;

    let manifest = add_owner_to_yaml_manifest(yaml_manifest, &project)?;

    let get_request = Request::builder()
        .uri(&path)
        .method("GET")
        .body("".into())
        .unwrap();

    let request;
    if client.request_text(get_request).await.is_ok() {
        // update resource
        request = Request::builder()
            .uri(format!(
                "{}?fieldManager=self-service-operator&force=true",
                &path
            ))
            .method("PATCH")
            .header("Content-Type", "application/apply-patch+yaml")
            .body(manifest.into())
            .unwrap();
    } else {
        // create resource
        let path = Path::new(&path).parent().unwrap();
        request = Request::builder()
            .uri(format!(
                "{}?fieldManager=self-service-operator&force=true",
                &path.display().to_string()
            ))
            .method("POST")
            .header("Content-Type", "application/yaml")
            .body(manifest.clone().into())
            .unwrap();
    }

    match client.request_text(request).await {
        Ok(_) => Ok(()),
        Err(e) => bail!("error applying manifest: {}", e),
    }
}

pub fn add_owner_to_yaml_manifest(
    yaml_manifest: &str,
    project: &Project,
) -> anyhow::Result<String> {
    let owner = serde_yaml::to_value(&OwnerReference::from(project));

    let mut yaml: serde_yaml::Value = serde_yaml::from_str(yaml_manifest)?;

    yaml["metadata"]["ownerReferences"] = serde_yaml::Value::Sequence(vec![owner.unwrap()]);

    let owned_manifest_as_string = serde_yaml::to_string(&yaml)?;

    Ok(owned_manifest_as_string)
}

pub async fn resource_path(client: &kube::Client, yaml_manifest: &str) -> anyhow::Result<String> {
    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ResourceInfo {
        metadata: ObjectMeta,
        api_version: String,
        kind: String,
    }

    let resource_info: ResourceInfo = serde_yaml::from_str(yaml_manifest)?;

    let is_core_api_resource = !resource_info.api_version.contains('/');

    let available_resources_in_current_cluster = if is_core_api_resource {
        client
            .list_core_api_resources(&(resource_info.api_version))
            .await?
    } else {
        client
            .list_api_group_resources(&(resource_info.api_version))
            .await?
    };

    let resource = available_resources_in_current_cluster
        .resources
        .into_iter()
        .find(|resource| resource.kind == resource_info.kind)
        .with_context(|| {
            format!(
                "api version {} not available in kubernetes cluster",
                resource_info.api_version
            )
        })?;

    let namespace_sub_path = if resource.namespaced {
        ensure!(
            resource_info.metadata.namespace.is_some(),
            "setting namespace is required: resource {}/{} with name '{}' has no namespace set ... in most cases you want to set it to {{{{ __PROJECT_NAME__ }}}}\nManifest is: {}",
            resource_info.api_version,
            resource_info.kind,
            resource_info.metadata.name.unwrap(),
            yaml_manifest);

        format!("namespaces/{}/", resource_info.metadata.namespace.unwrap())
    } else {
        "".to_string()
    };

    if is_core_api_resource {
        Ok(format!(
            "/api/{version}/{namespace_sub_path}{resource}/{name}",
            version = &resource_info.api_version,
            namespace_sub_path = namespace_sub_path,
            resource = &resource.name,
            name = resource_info.metadata.name.unwrap()
        ))
    } else {
        Ok(format!(
            "/apis/{api_version}/{namespace_sub_path}{resource}/{name}",
            api_version = &resource_info.api_version,
            namespace_sub_path = namespace_sub_path,
            resource = &resource.name,
            name = resource_info.metadata.name.unwrap()
        ))
    }
}