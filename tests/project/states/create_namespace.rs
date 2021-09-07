use std::time::Duration;

use anyhow::bail;
use k8s_openapi::api::core::v1::Namespace;
use kube::api::DeleteParams;
use serial_test::serial;
use tokio::select;
use tokio::time;

use self_service_operators::project::Project;

use crate::{project};
use crate::project::WaitForState;

#[tokio::test]
#[serial]
async fn it_creates_and_deletes_namespace() -> anyhow::Result<()> {
    let timeout_secs = 60;
    let (client, _) = project::before_each().await?;

    let name = project::random_name("namespace-test");
    let project = project::install_project(&client, &name).await?;

    let ns_api: kube::Api<Namespace> = kube::Api::all(client.clone());
    let project_namespace = ns_api.get(&name).await?;

    assert!(
        project::assert_is_owned_by_project(&project, &project_namespace).is_ok(),
        "namespace should be owned by project"
    );

    let wait_for_project_deleted_handle = project::wait_for_state(
        &kube::Api::<Project>::all(client.clone()),
        &name,
        WaitForState::Deleted,
    );

    let wait_for_namespace_deleted_handle =
        project::wait_for_state(&ns_api, &name, WaitForState::Deleted);

    assert!(
        kube::Api::<Project>::all(client.clone())
            .delete(&name, &DeleteParams::default())
            .await
            .is_ok(),
        "deleting project should work"
    );

    select! {
    res = futures::future::try_join(wait_for_project_deleted_handle,wait_for_namespace_deleted_handle) => {
        match res {
            Ok(_) => (),
            Err(e) => bail!("error deleting namespace {}: {}", name, e)
        }
    },
        _ = time::sleep(Duration::from_secs(timeout_secs)) => bail!("deleting project {} deletes project and namespace within {} seconds", name, timeout_secs)
    }

    Ok(())
}
