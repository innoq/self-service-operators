use futures::StreamExt;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::api;
use kube::config;
use kube::error::Error::ReqwestError;
use noqnoqnoq::self_service::project;
use pin_utils::pin_mut;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub async fn k8sclient() -> kube::Result<(config::Config, kube::Client)> {
    let kubeconfig = config::Kubeconfig::read_from(Path::new("./kind.kubeconfig"))?;
    let mut client_config =
        config::Config::from_custom_kubeconfig(kubeconfig, &config::KubeConfigOptions::default())
            .await?;

    client_config.timeout = Some(Duration::from_secs(1));
    let client = kube::Client::new(client_config.clone());

    // basic check so we fail early if k8s communication does not work
    client.apiserver_version().await?;
    Ok((client_config, client))
}

#[derive(Debug)]
pub enum WaitForState {
    Deleted,
    Created,
}

pub async fn reinstall_self_service_crd(client: &kube::Client) -> anyhow::Result<()> {
    let crds: kube::Api<CustomResourceDefinition> = kube::Api::all(client.clone());
    let self_service_project_crd_name = project::Project::crd().metadata.name.unwrap();

    match crds.get(self_service_project_crd_name.as_ref()).await {
        Ok(_) => {
            let delete_handle;
            {
                let client = client.clone();
                let self_service_project_crd_name = self_service_project_crd_name.clone();
                delete_handle = tokio::spawn(async move {
                    wait_for_state::<CustomResourceDefinition>(
                        client,
                        self_service_project_crd_name,
                        WaitForState::Deleted,
                    )
                    .await
                });
            }

            crds.delete(
                &self_service_project_crd_name,
                &api::DeleteParams {
                    propagation_policy: Some(api::PropagationPolicy::Foreground),
                    ..api::DeleteParams::default()
                },
            )
            .await?;
            let _ = delete_handle.await?;
        }
        _ => {}
    }

    let create_handle;
    {
        let client = client.clone();
        let self_service_project_crd_name = self_service_project_crd_name.clone();
        create_handle = tokio::spawn(async move {
            wait_for_state::<CustomResourceDefinition>(
                client,
                self_service_project_crd_name,
                WaitForState::Created,
            )
            .await
        });
    }
    noqnoqnoq::helper::install_crd(&client, &project::Project::crd()).await?;
    let _ = create_handle.await?;

    Ok(())
}

pub async fn wait_for_state<K: 'static>(
    client: kube::Client,
    name: String,
    state: WaitForState,
) -> anyhow::Result<()>
where
    K: k8s_openapi::Resource
        + Clone
        + std::marker::Send
        + kube::api::Meta
        + for<'de> serde::de::Deserialize<'de>,
{
    let api: kube::Api<K> = kube::Api::all(client.clone());
    let store_w = kube_runtime::reflector::store::Writer::default();
    let reflector = kube_runtime::reflector(
        store_w,
        kube_runtime::watcher(api, api::ListParams::default().timeout(5)),
    );

    // Use try_for_each to fail on first error, use for_each to keep retrying
    pin_mut!(reflector);
    println!(
        "{} with name {} waiting for state {:?}",
        K::KIND,
        name,
        state
    );
    while let Some(event) = reflector.next().await {
        match event {
            Ok(kube_runtime::watcher::Event::Deleted(_)) => match state {
                WaitForState::Deleted => break,
                _ => println!("    {} with name {} event Deleted", K::KIND, name),
            },
            Ok(kube_runtime::watcher::Event::Applied(_)) => match state {
                WaitForState::Created => break,
                _ => println!("    {} with name {} event Applied", K::KIND, name),
            },
            Ok(kube_runtime::watcher::Event::Restarted(_)) => {
                println!("    {} with name {} event Restarted", K::KIND, name)
            }
            Err(kube_runtime::watcher::Error::WatchFailed {
                source: ReqwestError(e),
                ..
            }) => {
                if Some(http::StatusCode::NOT_FOUND) == e.status() {
                    match state {
                        WaitForState::Deleted => break,
                        _ => println!("    {} with name {} event Error: {:?}", K::KIND, name, e),
                    }
                }
            }
            Err(e) => {
                println!("    {} with name {} event Error: {:?}", K::KIND, name, e)
            }
        }
    }

    // I know it sucks, but the k8s api seems to be eventual consistent ... relying on the
    // events is not working :/
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    println!("{} with name {} reached state {:?}", K::KIND, name, state);
    Ok(())
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
