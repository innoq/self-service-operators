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

use crate::project::operator::ProjectOperatorState;
use crate::project::project::{
    ONE_SHOT_MANIFEST_ANNOTATION_KEY, ONE_SHOT_MANIFEST_ANNOTATION_VALUE_ONCE,
};
use crate::project::project_status::ProjectStatus;
use crate::project::states::Error;
use crate::project::states::{ProjectPhase, ProjectState, WaitForChanges};
use crate::project::Project;
use serde_yaml::Value;
use std::ops::Mul;

#[derive(Debug, Default)]
pub(crate) struct ApplyManifests;

#[async_trait::async_trait]
impl State<ProjectState> for ApplyManifests {
    async fn next(
        self: Box<Self>,
        shared: Arc<RwLock<ProjectOperatorState>>,
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
                    if let Err(e) =
                        apply_yaml_manifest(&shared.client, manifest, &project, state).await
                    {
                        if i >= max_retries {
                            state.error = format!(
                                "error installing manifest: giving up after {} retries: {}\nmanifest was:\n{}",
                                i,
                                e,
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
                        manifests.insert(0, (i + 1, manifest));
                    }
                }
            }
        }

        Transition::next(self, WaitForChanges)
    }

    async fn status(
        &self,
        state: &mut ProjectState,
        project: &Project,
    ) -> anyhow::Result<ProjectStatus> {
        debug!("status() in ApplyManifests");

        let applied_one_shot_resources = (project.status.clone() as Option<ProjectStatus>).unwrap_or_default()
            .applied_one_shot_resources
            .into_iter()
            .collect();

        Ok(ProjectStatus {
            phase: Some(ProjectPhase::ApplyingManifests),
            message: Some("applying configured manifests".to_string()),
            summary: Some("applying configured manifests".to_string()),
            applied_one_shot_resources: (&applied_one_shot_resources
                | &state.applied_one_shot_resources)
                .into_iter()
                .collect(),
        })
    }
}

const FIELD_MANAGER_QUERY_ARG: &str = "fieldManager=self-service-operator&force=true";

pub async fn apply_yaml_manifest(
    client: &kube::Client,
    yaml_manifest: &str,
    project: &Project,
    state: &mut ProjectState,
) -> anyhow::Result<()> {
    let path = resource_path(client, yaml_manifest).await?;

    let is_one_shot_resource = is_one_shot_resource(yaml_manifest)?;

    let applied_one_shot_resources = (project.status.clone() as Option<ProjectStatus>).unwrap_or_default()
        .applied_one_shot_resources;

    if is_one_shot_resource && applied_one_shot_resources.contains(&path) {
        info!("one shot resource {} already applied, skipping", &path);
        return Ok(());
    }

    let manifest = add_owner_to_yaml_manifest(yaml_manifest, project)?;

    let get_request = Request::builder()
        .uri(&path)
        .method("GET")
        .body("".into())
        .unwrap();

    let request;
    if client.request_text(get_request).await.is_ok() {
        // update resource
        request = Request::builder()
            .uri(format!("{}?{}", &path, FIELD_MANAGER_QUERY_ARG))
            .method("PATCH")
            .header("Content-Type", "application/apply-patch+yaml")
            .body(manifest.into())
            .unwrap();
    } else {
        // create resource
        let path = Path::new(&path).parent().unwrap();
        request = Request::builder()
            .uri(format!(
                "{}?{}",
                &path.display().to_string(),
                FIELD_MANAGER_QUERY_ARG
            ))
            .method("POST")
            .header("Content-Type", "application/yaml")
            .body(manifest.clone().into())
            .unwrap();
    }

    match client.request_text(request).await {
        Ok(_) => {
            if is_one_shot_resource {
                state.applied_one_shot_resources.insert(path.to_string());
            }
            Ok(())
        }
        Err(e) => bail!("error applying manifest: {}", e.to_string()),
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

pub fn is_one_shot_resource(yaml_manifest: &str) -> anyhow::Result<bool> {
    let yaml: serde_yaml::Value = serde_yaml::from_str(yaml_manifest)?;

    if let serde_yaml::Value::Mapping(annotations) = &yaml["metadata"]["annotations"] {
        if annotations.get(&Value::String(ONE_SHOT_MANIFEST_ANNOTATION_KEY.to_string()))
            == Some(&Value::String(
                ONE_SHOT_MANIFEST_ANNOTATION_VALUE_ONCE.to_string(),
            ))
        {
            return Ok(true);
        }
    }

    Ok(false)
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
