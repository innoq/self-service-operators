use crate::common::WaitForState;
use k8s_openapi::api::core::v1::Namespace;
use kube::api::PostParams;
use noqnoqnoq::project;
use noqnoqnoq::self_service::Sample;
use std::time::Duration;
use tokio::select;
use tokio::time;

mod common;

#[tokio::test]
async fn it_creates_namespace() -> anyhow::Result<()> {
    let (_config, client) = common::before_each().await?;

    let name = common::random_name("project");

    let api: kube::Api<project::Project> = kube::Api::all(client.clone());
    let project = project::Project::new(name.as_str(), project::ProjectSpec::sample());

    let wait_for_project_handle =
        common::wait_for_state::<project::Project>(&client, &name, WaitForState::Created);
    assert!(
        api.create(&PostParams::default(), &project).await.is_ok(),
        "creating a new self service project should work correclty"
    );
    println!("BEFORE WAITING FOR SPAWN HANDLE");
    assert!(
        wait_for_project_handle.await.is_ok(),
        "expected project resource to be created successfully"
    );

    let namespace_create_handle =
        common::wait_for_state::<Namespace>(&client, &name, common::WaitForState::Created);
    assert!(
        select! {
            _ = namespace_create_handle => true,
            _ = time::sleep(Duration::from_secs(5)) => false,
        },
        "expected project related namespace to be created within 5 seconds"
    );

    Ok(())
}
