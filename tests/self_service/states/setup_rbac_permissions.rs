use core::time::Duration;

use k8s_openapi::api::rbac::v1::{ClusterRole, ClusterRoleBinding, RoleBinding};
use kube::Resource;
use serial_test::serial;
use tokio::select;
use tokio::time;

use noqnoqnoq::self_service::project;
use noqnoqnoq::self_service::project::Project;

use crate::common;
use crate::common::WaitForState;

#[tokio::test]
#[serial]
async fn it_should_create_clusterrole_and_clusterrolebinding_for_handling_this_project(
) -> anyhow::Result<()> {
    let (client, _) = common::before_each().await?;
    let timeout_secs = 60;
    let name = common::random_name("owner-cluster-role-test");

    let project = common::install_project(&client, &name).await?;

    let cr_api = kube::Api::<ClusterRole>::all(client.clone());
    let crb_api = kube::Api::<ClusterRoleBinding>::all(client.clone());

    let wait_for_clusterrole_created_handle = common::wait_for_state(
        &cr_api,
        &project.owner_cluster_role_name(),
        WaitForState::Created,
    );

    let wait_for_clusterrolebinding_created_handle = common::wait_for_state(
        &crb_api,
        &project.owner_cluster_role_name(),
        WaitForState::Created,
    );

    assert!(
        select! {
        res = wait_for_clusterrole_created_handle => res.is_ok(),
        _ = time::sleep(Duration::from_secs(timeout_secs)) => false
        },
        "clusterrole for the owner should be created within {} seconds",
        timeout_secs
    );

    assert!(
        select! {
        res = wait_for_clusterrolebinding_created_handle => res.is_ok(),
        _ = time::sleep(Duration::from_secs(timeout_secs)) => false
        },
        "clusterrolebinding for the owner should be created within {} seconds",
        timeout_secs
    );

    let cr = cr_api.get(&project.owner_cluster_role_name()).await?;
    assert!(
        common::assert_is_owned_by_project(&project, &cr).is_ok(),
        "owner cluster role should be owned by project"
    );

    assert!(cr.rules.is_some(), "cluster role should have rules");
    let rule = &cr.rules.unwrap()[0];
    assert_eq!(
        rule.api_groups,
        Some(vec![Project::group(&()).to_string()]),
        "owner cluster role should have correct api group set"
    );
    assert_eq!(
        rule.resource_names,
        Some(vec![project.metadata.name.as_ref().unwrap().to_owned()]),
        "owner cluster role should limit role to this specific project"
    );
    assert_eq!(
        rule.resources,
        Some(vec!["projects".to_string()]),
        "owner cluster role should have correct resource set"
    );

    assert_eq!(
        rule.verbs,
        vec![
            "get".to_string(),
            "list".to_string(),
            "watch".to_string(),
            "create".to_string(),
            "update".to_string(),
            "patch".to_string(),
            "delete".to_string(),
        ],
        "owner cluster role should have correct verbs set"
    );

    let crb = crb_api.get(&project.owner_cluster_role_name()).await?;
    assert!(
        common::assert_is_owned_by_project(&project, &crb).is_ok(),
        "owner cluster role binding should be owned by project"
    );

    assert_eq!(
        crb.role_ref.kind, "ClusterRole",
        "owner rolebinding role-ref kind should be ClusterRole"
    );

    assert_eq!(
        crb.role_ref.name,
        project.owner_cluster_role_name(),
        "owner rolebinding role-ref name should be correct"
    );

    assert!(
        crb.subjects.is_some(),
        "cluster role binding should have subjects"
    );

    let subject = &crb.subjects.unwrap()[0];

    assert_eq!(
        subject.kind, "User",
        "cluster role binding subject kind should be correct"
    );
    assert_eq!(
        subject.name, project.spec.owners[0],
        "cluster role binding subject name should be correct"
    );

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
async fn it_creates_rolebinding() -> anyhow::Result<()> {
    let (client, _) = common::before_each().await?;
    let timeout_secs = 6;
    let name = common::random_name("rolebinding-test");

    let project = common::install_project(&client, &name).await?;

    let api = kube::Api::<RoleBinding>::namespaced(client.clone(), name.as_str());
    let wait_for_rolebinding_created_handle = common::wait_for_state(
        &api,
        &project::OWNER_ROLE_BINDING_NAME.to_string(),
        WaitForState::Created,
    );

    assert!(
        select! {
        res = wait_for_rolebinding_created_handle => res.is_ok(),
        _ = time::sleep(Duration::from_secs(timeout_secs)) => false
        },
        "rolebinding for the owner should be created within {} seconds",
        timeout_secs
    );

    let rb = api.get(&project::OWNER_ROLE_BINDING_NAME).await?;

    assert!(
        common::assert_is_owned_by_project(&project, &rb).is_ok(),
        "owner role binding should be owned by project"
    );

    assert_eq!(
        rb.metadata.name.unwrap(),
        project::OWNER_ROLE_BINDING_NAME,
        "owner rolebinding name should be correct"
    );
    assert!(
        rb.subjects.is_some() && !rb.subjects.as_ref().unwrap().is_empty(),
        "owner rolebinding subject should exit"
    );

    let subject = &rb.subjects.as_ref().unwrap()[0];
    assert_eq!(
        subject.name, project.spec.owners[0],
        "subject name should be correct"
    );

    let subject1 = &rb.subjects.as_ref().unwrap()[1];
    assert_eq!(
        subject1.name, project.spec.owners[1],
        "subject name should be correct"
    );

    assert_eq!(
        subject.kind,
        "User".to_string(),
        "subject kind should be correct"
    );

    assert_eq!(rb.role_ref.name, "admin", "role-ref name should be correct");
    assert_eq!(
        rb.role_ref.api_group, "rbac.authorization.k8s.io",
        "role-ref api groups should be correct"
    );
    assert_eq!(
        rb.role_ref.kind, "ClusterRole",
        "role-ref kind should be correct"
    );

    assert!(
        common::assert_project_is_in_waiting_state(&client, &name)
            .await
            .is_ok(),
        "project should be in waiting state"
    );

    Ok(())
}
