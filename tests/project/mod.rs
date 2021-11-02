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

use log::debug;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::{select, time};

use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::admissionregistration::v1::MutatingWebhookConfiguration;
use k8s_openapi::api::core::v1::{Namespace, Secret, Service};
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use krator::OperatorRuntime;
use kube::api::{DeleteParams, Patch, PatchParams, PostParams, WatchEvent};
use kube::{api, config, Client};
use tokio::task::JoinHandle;

use self_service_operators::project::operator::ProjectOperator;
use self_service_operators::project::project::{
    DEFAULT_MANIFESTS_SECRET, SECRET_ANNOTATION_KEY, SECRET_ANNOTATION_VALUE,
};
use self_service_operators::project::{ProjectSpec, ProjectStatus, Sample};

use self_service_operators::project::states::ProjectPhase;
use self_service_operators::project::Project;
use std::convert::TryFrom;

mod admission_webhook_tests;
mod manifest_secrets;
mod operator;
mod project;
mod states;
mod yaml_manifest_parsing;

pub async fn before_each() -> anyhow::Result<(kube::Client, ProjectOperator)> {
    let (config, client) = get_client().await?;

    assert!(
        apply_manifest_secret(
            &client,
            DEFAULT_MANIFESTS_SECRET,
            vec![
                include_str!("../fixtures/pod.yaml"),
                include_str!("../../manifests/default-project-manifests/owner-clusterrole.yaml"),
                include_str!(
                    "../../manifests/default-project-manifests/owner-clusterrolebinding.yaml"
                ),
                include_str!(
                    "../../manifests/default-project-manifests/project-owner-role-binding.yaml"
                ),
            ]
        )
        .await
        .is_ok(),
        "installing default manifest secret should work"
    );

    // there is probably a better way FnOnce?
    let _ = reinstall_self_service_crd(&client).await?;

    let operator = self_service_operators::project::operator::ProjectOperator::new(
        client.clone(),
        "default",
        DEFAULT_MANIFESTS_SECRET,
        Duration::from_secs(0),
    )
    .await
    .unwrap();
    let mut runtime = OperatorRuntime::new(&config, operator.clone(), None);

    tokio::spawn(async move { runtime.start().await });

    Ok((client, operator))
}

pub async fn get_client() -> Result<(kube::Config, kube::Client), anyhow::Error> {
    let mut kube_config = config::Kubeconfig::read_from(Path::new("./kind.kubeconfig"))?;
    kube_config.clusters[0].cluster.server = kube_config.clusters[0]
        .cluster
        .server
        .replace("127.0.0.1", "localhost");
    let mut config =
        config::Config::from_custom_kubeconfig(kube_config, &config::KubeConfigOptions::default())
            .await?;
    config.timeout = Some(Duration::from_secs(10));
    let client = kube::Client::try_from(config.clone())?;
    assert!(
        client.apiserver_version().await.is_ok(),
        "communication with kubernetes should work"
    );
    Ok((config, client))
}

#[derive(Debug, Clone, Copy)]
pub enum WaitForState {
    Deleted,
    Created,
    Updated,
}

pub async fn reinstall_self_service_crd(client: &kube::Client) -> anyhow::Result<()> {
    let api: kube::Api<CustomResourceDefinition> = kube::Api::all(client.clone());
    let name = Project::crd().metadata.name.unwrap();

    if api.get(&name).await.is_ok() {
        let wait_for_crd_deleted = wait_for_state(&api, &name, WaitForState::Deleted);

        api.delete(&name, &api::DeleteParams::default()).await?;
        let _ = wait_for_crd_deleted.await?;
    }

    let wait_for_crd_created = wait_for_state(&api, &name, WaitForState::Created);
    let crd = self_service_operators::install_crd(&client, &Project::crd()).await?;
    let _ = wait_for_crd_created.await?;

    const NAMESPACE: &str = "default";
    let (service, secret, config) = Project::admission_webhook_resources(NAMESPACE);

    {
        let api = kube::Api::<Service>::namespaced(client.clone(), NAMESPACE);
        let service_name = service.metadata.name.as_ref().unwrap();
        if api.get(service_name).await.is_ok() {
            wait_for_state(&api, service_name, WaitForState::Deleted).await?;
        }
    }

    krator::admission::WebhookResources(service, secret, config.clone())
        .apply_owned(&client.clone(), &crd)
        .await?;

    // delete the webhook config again as we will not run within the cluster during testing
    {
        let api = kube::Api::<MutatingWebhookConfiguration>::all(client.clone());
        let config_name = config.metadata.name.as_ref().unwrap();
        let config_deletion = wait_for_state(&api, config_name, WaitForState::Deleted);
        api.delete(config_name, &DeleteParams::default()).await?;
        config_deletion.await?
    }

    Ok(())
}

pub fn wait_for_state<K: 'static>(
    api: &kube::Api<K>,
    #[allow(clippy::ptr_arg)] name: &String,
    state: WaitForState,
) -> JoinHandle<()>
where
    K: std::fmt::Debug
        + kube::Resource
        + Clone
        + std::marker::Send
        + for<'de> serde::de::Deserialize<'de>,
{
    let name = name.clone();
    let api = api.clone();

    tokio::spawn(async move {
        debug!(
            "{} with name {} waiting for state {:?}",
            api.resource_url(),
            name,
            state
        );

        let lp = &api::ListParams::default()
            .timeout(10)
            .fields(format!("metadata.name={}", name).as_str());

        let resource_version;
        loop {
            match api.list(&lp).await {
                Ok(list) => {
                    resource_version = list.metadata.resource_version.unwrap();
                    break;
                }
                _ => {
                    tokio::time::sleep(time::Duration::from_millis(100)).await;
                }
            }
        }

        // check whether state is already reached before starting a watch
        let get_res = api.get(&name).await;
        match state {
            WaitForState::Created if get_res.is_ok() => return,
            WaitForState::Deleted if get_res.is_err() => return,
            _ => {}
        }

        let mut stream = api.watch(lp, &resource_version).await.unwrap().boxed();

        let print_info = {
            |e: &WatchEvent<K>, resource: &K| {
                debug!(
                    "  - {:?} for {} with name {} received",
                    e,
                    api.resource_url(),
                    (resource.meta().clone() as ObjectMeta).name.unwrap(),
                );
            }
        };

        loop {
            match stream.try_next().await {
                Ok(Some(status)) => match status.clone() {
                    WatchEvent::Added(resource) => {
                        print_info(&status, &resource);
                        if let WaitForState::Created = state {
                            break;
                        }
                        if let WaitForState::Updated = state {
                            break;
                        }
                    }
                    WatchEvent::Bookmark(bookmark) => {
                        debug!(" - {:?} for {}", status, bookmark.types.kind);
                    }
                    WatchEvent::Modified(resource) => {
                        print_info(&status, &resource);
                        if let WaitForState::Updated = state {
                            break;
                        }
                    }
                    WatchEvent::Deleted(resource) => {
                        print_info(&status, &resource);
                        if let WaitForState::Deleted = state {
                            break;
                        }
                    }
                    WatchEvent::Error(e) => {
                        debug!(
                            " - ERROR watching {} with name {}: {:?}",
                            api.resource_url(),
                            name,
                            e
                        );
                    }
                },
                Ok(None) => {
                    // happens, if nothing watchable was found (e.g. watching for something in a namespace
                    // that does not exist yet
                    if let WaitForState::Deleted = state {
                        break;
                    }

                    let msg = format!(
                        "  - too early to watch {} with name {} (event: {:?})",
                        api.resource_url(),
                        &name,
                        &state
                    );
                    debug!("{}", msg);
                    tokio::time::sleep(time::Duration::from_millis(250)).await;
                    let _ = wait_for_state(&api, &name, state).await;
                    break;
                }
                Err(e) => {
                    debug!(
                        " - ERROR getting {} with name {} from stream: {:?}",
                        api.resource_url(),
                        name,
                        e
                    );
                }
            }
        }
        // again: Kubernetes-API does not seem to be strictly consistent ...
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    })
}

pub async fn install_project(
    client: &kube::Client,
    #[allow(clippy::ptr_arg)] name: &String,
) -> anyhow::Result<Project> {
    let _ = env_logger::builder()
        // Ensure events are captured by `cargo test`
        .is_test(true)
        // Ignore errors initializing the logger if tests race to configure it
        .try_init();

    let timeout_secs = 20;

    let wait_for_namespace_created_handle = wait_for_state(
        &kube::Api::<Namespace>::all(client.clone()),
        name,
        WaitForState::Created,
    );

    let project_api: kube::Api<Project> = kube::Api::all(client.clone());
    let wait_for_project_created_handle =
        wait_for_state(&project_api, &name, WaitForState::Created);

    let manifest_values = "name: templated-name";

    let mut spec = ProjectSpec::sample();
    spec.manifest_values = Some(manifest_values.into());

    let project = Project::new(name.as_str(), spec);

    let project_resource = project_api.create(&PostParams::default(), &project).await;

    assert!(
        project_resource.is_ok(),
        "creating a new self service project should work correctly: {}",
        project_resource.err().unwrap()
    );
    assert!(
        select! {
        res = futures::future::try_join(wait_for_namespace_created_handle, wait_for_project_created_handle) => res.is_ok(),
            _ = time::sleep(Duration::from_secs(timeout_secs)) => false,
        },
        "expected project related namespace {} to be created within {} seconds",
        name,
        timeout_secs
    );

    Ok(project_resource.unwrap())
}

pub fn random_name(prefix: &str) -> String {
    format!(
        "{}-{}",
        prefix,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    )
}

#[allow(dead_code)] // it's not used by every test and therefore sometimes throws warnings
pub fn assert_is_owned_by_project<R>(project: &Project, resource: &R) -> anyhow::Result<()>
where
    R: kube::Resource + k8s_openapi::Resource,
{
    assert!(
        resource.meta().owner_references.is_some(),
        "{} should have owner reference",
        R::KIND
    );

    let owners = resource.meta().owner_references.as_ref().unwrap();
    assert!(
        !owners.is_empty(),
        "{} should have at least one owner",
        R::KIND
    );

    let owner = &owners[0];
    assert_eq!(
        owner.api_version, project.api_version,
        "api_version of owner-reference is wrong"
    );
    assert_eq!(
        owner.controller,
        Some(true),
        "controller of owner-reference is wrong"
    );
    assert_eq!(owner.kind, project.kind, "kind of owner-reference is wrong");
    assert_eq!(
        owner.name,
        project.metadata.name.clone().unwrap(),
        "name of owner-reference is wrong"
    );
    assert_eq!(
        owner.uid,
        project.metadata.uid.clone().unwrap(),
        "uid of owner-reference is wrong: owner: {:#?}, project: {:#?}",
        owner,
        project
    );

    Ok(())
}

#[allow(dead_code)] // it's not used by every test and therefore sometimes throws warnings
pub async fn assert_project_is_in_phase(
    client: &Client,
    name: &str,
    phase: ProjectPhase,
) -> anyhow::Result<()> {
    let mut current_project_phase = None;

    let api: kube::Api<Project> = kube::Api::all(client.clone());
    for _ in 0..60 {
        let _ = tokio::time::sleep(Duration::from_secs(1)).await;
        let project = api.get(&name).await?;

        let status: &ProjectStatus = &project.status.clone().unwrap();
        current_project_phase = status.phase.clone();

        if current_project_phase.is_some()
            && std::mem::discriminant(current_project_phase.as_ref().unwrap())
                == std::mem::discriminant(&phase)
        {
            return Ok(());
        }
    }

    panic!(
        "project should be in phase {:?} but is in phase {:?}",
        phase, current_project_phase
    );
}

pub async fn apply_manifest_secret(
    client: &kube::Client,
    name: &str,
    manifests: Vec<&str>,
) -> anyhow::Result<(), anyhow::Error> {
    let api = kube::Api::<Secret>::namespaced(client.clone(), "default");

    if api.get(name).await.is_ok() {
        api.delete(
            name,
            &DeleteParams {
                ..Default::default()
            },
        )
        .await?;
    }

    let wait_for_secret_created_handle = wait_for_state(
        &kube::Api::<Secret>::all(client.clone()),
        &name.to_string(),
        WaitForState::Updated,
    );

    let mut annotations = BTreeMap::new();
    annotations.insert(
        SECRET_ANNOTATION_KEY.to_string(),
        SECRET_ANNOTATION_VALUE.to_string(),
    );

    let mut secret_items = BTreeMap::new();
    manifests.iter().enumerate().for_each(|(i, manifest)| {
        secret_items.insert(format!("resource{}", i), manifest.to_string());
    });

    api.patch(
        name,
        &PatchParams {
            force: false,
            field_manager: Some("operator-test".to_string()),
            ..Default::default()
        },
        &Patch::Apply(&Secret {
            data: None,
            string_data: Some(secret_items),
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                annotations: Some(annotations),
                ..Default::default()
            },
            ..Default::default()
        }),
    )
    .await?;
    let _ = wait_for_secret_created_handle.await;
    Ok(())
}
