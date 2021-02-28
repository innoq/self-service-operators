use futures::StreamExt;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use krator::OperatorRuntime;
use kube::api;
use kube::config;
use kube::error::Error::ReqwestError;
use noqnoqnoq::self_service::project;
use pin_utils::pin_mut;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::task::JoinHandle;

pub async fn before_each() -> anyhow::Result<(config::Config, kube::Client)> {
    let kubeconfig = config::Kubeconfig::read_from(Path::new("./kind.kubeconfig"))?;
    let mut config =
        config::Config::from_custom_kubeconfig(kubeconfig, &config::KubeConfigOptions::default())
            .await?;
    config.timeout = Some(Duration::from_secs(1));

    let client = kube::Client::new(config.clone());

    // basic check so we fail early if k8s communication does not work
    assert!(
        client.apiserver_version().await.is_ok(),
        "communication with kubernetes should work"
    );

    // there is probably a better way FnOnce?
    static mut INITIALIZED: bool = false;
    unsafe {
        if !INITIALIZED {
            let _ = reinstall_self_service_crd(&client).await?;

            let mut runtime = OperatorRuntime::new(&config, project::ProjectOperator::new(), None);

            tokio::spawn(async move { runtime.start().await });
            INITIALIZED = true;
        }
    }

    Ok((config, client))
}

#[derive(Debug)]
pub enum WaitForState {
    Deleted,
    Created,
}

pub async fn reinstall_self_service_crd(client: &kube::Client) -> anyhow::Result<()> {
    let api: kube::Api<CustomResourceDefinition> = kube::Api::all(client.clone());
    let name = project::Project::crd().metadata.name.unwrap();

    match api.get(&name).await {
        Ok(_) => {
            let wait_for_crd_deleted =
                wait_for_state::<CustomResourceDefinition>(&client, &name, WaitForState::Deleted);

            api.delete(&name, &api::DeleteParams::default()).await?;
            let _ = wait_for_crd_deleted.await?;
        }
        _ => {}
    }

    noqnoqnoq::helper::install_crd(&client, &project::Project::crd()).await?;

    // TODO: check ... k8s api reports delete event before it can accept resources of this type
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    Ok(())
}

pub fn wait_for_state<K: 'static>(
    client: &kube::Client,
    name: &String,
    state: WaitForState,
) -> JoinHandle<()>
where
    K: k8s_openapi::Resource
        + Clone
        + std::marker::Send
        + kube::api::Meta
        + for<'de> serde::de::Deserialize<'de>
        + Sync,
{
    let client = client.clone();
    let name = name.clone();

    tokio::spawn(async move {
        let api: kube::Api<K> = kube::Api::all(client.clone());
        let store_w = kube_runtime::reflector::store::Writer::default();
        let reflector = kube_runtime::reflector(
            store_w,
            kube_runtime::watcher(api, api::ListParams::default().timeout(5)),
        );

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
                            _ => {
                                println!("    {} with name {} event Error: {:?}", K::KIND, name, e)
                            }
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
        println!(
            "    {} with name {} reached state {:?}",
            K::KIND,
            name,
            state
        );
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
