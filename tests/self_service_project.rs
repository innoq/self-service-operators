use crate::common::WaitForState;
use futures::join;
use futures::try_join;
use k8s_openapi::api::core::v1::Namespace;
use kube::api::{DeleteParams, PostParams};
use noqnoqnoq::project;
use noqnoqnoq::self_service::Sample;
use std::time::Duration;
use tokio::select;
use tokio::time;

mod common;

#[tokio::test]
async fn it_creates_namespace() -> anyhow::Result<()> {
    let timeout_secs = 10;
    let (_config, client) = common::before_each().await?;

    let name = common::random_name("project");
    let project_api: kube::Api<project::Project> = kube::Api::all(client.clone());

    let wait_for_project_created_handle =
        common::wait_for_state::<project::Project>(&client, &name, WaitForState::Created);
    let wait_for_namespace_created_handle =
        common::wait_for_state::<Namespace>(&client, &name, common::WaitForState::Created);

    let project = project::Project::new(name.as_str(), project::ProjectSpec::sample());
    assert!(
        project_api
            .create(&PostParams::default(), &project)
            .await
            .is_ok(),
        "creating a new self service project should work correclty"
    );

    // assert!(
    //     wait_for_project_created_handle.await.is_ok(),
    //     "expected project resource to be created successfully"
    // );

    assert!(
        select! {
        res = futures::future::try_join(wait_for_namespace_created_handle, wait_for_project_created_handle) => res.is_ok(),
            _ = time::sleep(Duration::from_secs(timeout_secs)) => false,
        },
        "expected project related namespace {} to be created within {} seconds",
        name,
        timeout_secs
    );

    let ns_api: kube::Api<Namespace> = kube::Api::all(client.clone());
    let new_namespace = ns_api.get(&name).await?;

    assert!(
        new_namespace.metadata.owner_references.is_some(),
        "namespace should have owner reference"
    );
    let owners = new_namespace.metadata.owner_references.unwrap();
    assert!(owners.len() > 0, "namespace should have at least one owner");

    let owner = &owners[0];
    assert!(
        owner.name == name && owner.kind == project.kind,
        "namespacefutures::future::try_joinr reference should point to the new project"
    );

    let wait_for_project_deleted_handle =
        common::wait_for_state::<project::Project>(&client, &name, WaitForState::Deleted);

    let wait_for_namespace_deleted_handle =
        common::wait_for_state::<Namespace>(&client, &name, WaitForState::Deleted);

    assert!(
        project_api
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
