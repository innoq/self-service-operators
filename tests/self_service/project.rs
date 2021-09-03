use core::option::Option::None;
use core::result::Result::{Err, Ok};
use core::time::Duration;

use kube::api::PostParams;
use kube::{Resource, ResourceExt};
use serial_test::serial;

use noqnoqnoq::self_service::project::{Project, ProjectSpec};
use noqnoqnoq::self_service::{operator, project};

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

#[tokio::test]
#[serial]
async fn it_fails_with_non_existant_owner_default_role_binding() -> anyhow::Result<()> {
    let (_, client) = common::get_client().await?;

    assert!(
        common::apply_manifest_secret(
            &client,
            project::DEFAULT_MANIFESTS_SECRET,
            vec![include_str!("../fixtures/pod.yaml")]
        )
        .await
        .is_ok(),
        "installing default manifest secret should work"
    );

    match operator::ProjectOperator::new(
        client.clone(),
        "non-existant-cluster-role-name",
        "default",
        project::DEFAULT_MANIFESTS_SECRET,
        Duration::from_secs(0),
    )
    .await
    {
        Ok(_) => panic!(
            "project operator should fail if the given default owner cluster role does not exist"
        ),
        Err(e) => assert_eq!(
            e.to_string(),
            "no ClusterRole with name 'non-existant-cluster-role-name' found -- aborting",
            "error message should be correct"
        ),
    };
    Ok(())
}
