use crate::common::WaitForState;
use k8s_openapi::api::core::v1::Namespace;
use kube::api::DeleteParams;
use noqnoqnoq::project;
use serial_test::serial;
use std::time::Duration;
use tokio::select;
use tokio::time;

mod common;

#[tokio::test]
#[serial]
async fn it_creates_namespace() -> anyhow::Result<()> {
    let timeout_secs = 10;
    let client = common::before_each().await?;

    let (project, name) = common::install_project(&client, "namespace-test").await?;

    let ns_api: kube::Api<Namespace> = kube::Api::all(client.clone());

    let new_namespace = ns_api.get(&name).await?;

    let _ = common::is_owned_by_project(&project, &new_namespace);

    let wait_for_project_deleted_handle = common::wait_for_state(
        &kube::Api::<project::Project>::all(client.clone()),
        &name,
        WaitForState::Deleted,
    );

    let wait_for_namespace_deleted_handle =
        common::wait_for_state(&ns_api, &name, WaitForState::Deleted);

    assert!(
        kube::Api::<project::Project>::all(client.clone())
            .delete(name.as_str(), &DeleteParams::default())
            .await
            .is_ok(),
        "deleting project should work"
    );

    assert!(
        select! {
        res = futures::future::try_join(wait_for_project_deleted_handle,wait_for_namespace_deleted_handle) => res.is_ok(),
            _ = time::sleep(Duration::from_secs(timeout_secs)) => false,
        },
        "deleting project {} deletes project and namespace within {} seconds",
        name,
        timeout_secs
    );

    Ok(())
}

#[tokio::test]
#[serial]
async fn it_creates_rolebinding() -> anyhow::Result<()> {
    let client = common::before_each().await?;

    let (_project, name) = common::install_project(&client, "rolebinding-test").await?;

    // let rb_api = kube::Api::<RoleBinding>::namespaced(client.clone(), name.as_str());

    kube::Api::<project::Project>::all(client.clone())
        .delete(name.as_str(), &DeleteParams::default())
        .await?;

    Ok(())
}
