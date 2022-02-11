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

use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::admissionregistration::v1::MutatingWebhookConfiguration;
use k8s_openapi::api::core::v1::{Secret, Service};
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, Patch, PatchParams, WatchEvent};
use kube::{api, config};
use log::debug;
use self_service_operators::project::project::{SECRET_ANNOTATION_KEY, SECRET_ANNOTATION_VALUE};
use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::path::Path;
use std::time;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::task::JoinHandle;

mod postgres;
mod project;

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

pub async fn reinstall_self_service_crd(
    client: &kube::Client,
    crd: &CustomResourceDefinition,
    webhook_resources: (Service, Secret, MutatingWebhookConfiguration),
) -> anyhow::Result<()> {
    let api: kube::Api<CustomResourceDefinition> = kube::Api::all(client.clone());
    let name = crd.metadata.name.as_ref().unwrap();

    if api.get(name).await.is_ok() {
        let wait_for_crd_deleted = wait_for_state(&api, name, WaitForState::Deleted);

        api.delete(name, &crate::DeleteParams::default()).await?;
        let _ = wait_for_crd_deleted.await?;
    }

    let wait_for_crd_created = wait_for_state(&api, name, WaitForState::Created);
    let crd = self_service_operators::install_crd(client, crd).await?;
    wait_for_crd_created.await?;

    const NAMESPACE: &str = "default";
    let (service, secret, config) = webhook_resources;

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

#[derive(Debug, Clone, Copy)]
pub enum WaitForState {
    Deleted,
    Created,
    Updated,
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
            match api.list(lp).await {
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
