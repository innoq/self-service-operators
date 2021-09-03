use core::option::Option::None;
use core::result::Result::{Err, Ok};

use kube::api::PostParams;
use kube::{Resource, ResourceExt};
use serial_test::serial;

use noqnoqnoq::self_service::project::{Project, ProjectSpec};

use crate::common;

#[tokio::test]
#[serial]
async fn it_is_possible_to_update_project() -> anyhow::Result<()> {
    let (client, _) = common::before_each().await?;

    let name = common::random_name("update-project");
    let _ = common::install_project(&client, &name).await?;

    let api: kube::Api<Project> = kube::Api::all(client.clone());

    let _ = common::assert_project_is_in_waiting_state(&client, &name).await;

    let mut project = api.get(&name).await?;
    let resource_version = project.resource_version();
    project.spec = ProjectSpec {
        owners: vec!["newowner@example.com".to_string()],
        manifest_values: project.spec.manifest_values,
    };
    let meta = project.meta_mut();
    meta.resource_version = resource_version;
    meta.managed_fields = None;

    if let Err(e) = api.replace(&name, &PostParams::default(), &project).await {
        panic!("error updating project: {:?}:\n{}", &project, e);
    }

    assert!(
        common::assert_project_is_in_waiting_state(&client, &name)
            .await
            .is_ok(),
        "project should be in waiting state"
    );

    Ok(())
}
