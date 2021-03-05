use crate::common::WaitForState;
use k8s_openapi::api::core::v1::Namespace;
use k8s_openapi::api::rbac::v1::{ClusterRole, ClusterRoleBinding, RoleBinding};
use kube::api::DeleteParams;
use kube::config;
use noqnoqnoq::project;
use serial_test::serial;
use std::path::Path;
use std::time::Duration;
use tokio::select;
use tokio::time;

mod common;

#[tokio::test]
#[serial]
async fn it_creates_namespace() -> anyhow::Result<()> {
    let timeout_secs = 10;
    let client = common::before_each().await?;

    let name = common::random_name("namespace-test");
    let project = common::install_project(&client, &name).await?;

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
#[ignore = "not yet implemented"]
async fn it_should_not_fail_if_namespace_was_already_created_by_project() -> anyhow::Result<()> {
    // later: we can't check the status atm
    Ok(())
}

#[tokio::test]
#[serial]
#[ignore = "not yet implemented"]
async fn it_should_fail_if_namespace_already_exists_but_was_not_created_by_this_operator(
) -> anyhow::Result<()> {
    // later: we can't check the status atm
    Ok(())
}

#[tokio::test]
#[serial]
async fn it_should_create_clusterrole_and_clusterrolebinding_for_handling_this_project_cr(
) -> anyhow::Result<()> {
    let client = common::before_each().await?;
    let timeout_secs = 6;
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
        common::is_owned_by_project(&project, &cr).is_ok(),
        "owner cluster role should be owned by project"
    );

    assert!(cr.rules.is_some(), "cluster role should have rules");
    let rule = &cr.rules.unwrap()[0];
    // assert_eq!(
    //     rule.api_groups,
    //     Some(vec![project.group]),
    //     "owner cluster role should have correct api group set"
    // );
    assert_eq!(
        rule.resource_names,
        Some(vec![project.metadata.name.clone().unwrap()]),
        "owner cluster role should limit role to this specific project"
    );
    assert_eq!(
        rule.resources,
        Some(vec![project.clone().kind]),
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
        common::is_owned_by_project(&project, &crb).is_ok(),
        "owner cluster role binding should be owned by project"
    );

    assert_eq!(
        crb.role_ref.kind,
        "ClusterRole".to_string(),
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
        subject.kind,
        "User".to_string(),
        "cluster role binding subject kind should be correct"
    );
    assert_eq!(
        subject.name, project.spec.owner,
        "cluster role binding subject name should be correct"
    );

    kube::Api::<project::Project>::all(client.clone())
        .delete(name.as_str(), &DeleteParams::default())
        .await?;

    Ok(())
}

#[tokio::test]
#[serial]
async fn it_fails_with_non_existant_owner_default_role_binding() -> anyhow::Result<()> {
    let kubeconfig = config::Kubeconfig::read_from(Path::new("./kind.kubeconfig"))?;
    let config =
        config::Config::from_custom_kubeconfig(kubeconfig, &config::KubeConfigOptions::default())
            .await?;

    let client = kube::Client::new(config.clone());
    match project::ProjectOperator::new(
        client.clone(),
        "non-existant-cluster-role-name".to_string(),
    )
    .await
    {
        Ok(_) => assert!(
            false,
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

#[tokio::test]
#[serial]
async fn it_creates_rolebinding() -> anyhow::Result<()> {
    let client = common::before_each().await?;
    let timeout_secs = 6;
    let name = common::random_name("rolebinding-test");

    let project = common::install_project(&client, &name).await?;

    let rb_api = kube::Api::<RoleBinding>::namespaced(client.clone(), name.as_str());
    let wait_for_rolebinding_created_handle = common::wait_for_state(
        &rb_api,
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

    let rb = rb_api.get(&project::OWNER_ROLE_BINDING_NAME).await?;

    assert!(
        common::is_owned_by_project(&project, &rb).is_ok(),
        "owner role binding should be owned by project"
    );

    assert_eq!(
        rb.metadata.name.unwrap(),
        project::OWNER_ROLE_BINDING_NAME,
        "owner rolebinding name should be correct"
    );
    assert!(
        rb.subjects.is_some() && rb.subjects.clone().unwrap().len() > 0,
        "owner rolebinding subject should exit"
    );

    let subject = &rb.subjects.unwrap()[0];
    assert_eq!(
        subject.name, project.spec.owner,
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

    kube::Api::<project::Project>::all(client.clone())
        .delete(name.as_str(), &DeleteParams::default())
        .await?;

    Ok(())
}
