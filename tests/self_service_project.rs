use crate::common::WaitForState;
use anyhow::bail;
use config::{Config, Kubeconfig};
use k8s_openapi::api::core::v1::{Namespace, Pod, Secret, ServiceAccount};
use k8s_openapi::api::rbac::v1::{ClusterRole, ClusterRoleBinding, RoleBinding};
use krator::{admission::AdmissionResult, Operator};

use kube::api::DeleteParams;
use kube::{config, Resource};
use noqnoqnoq::project::{self, Project};
use serial_test::serial;
use std::time::Duration;
use std::{convert::TryFrom, path::Path};
use tokio::select;
use tokio::time;

mod common;

#[tokio::test]
#[serial]
async fn it_creates_and_deletes_namespace() -> anyhow::Result<()> {
    let timeout_secs = 60;
    let (client, _) = common::before_each().await?;

    let name = common::random_name("namespace-test");
    let project = common::install_project(&client, &name).await?;

    let ns_api: kube::Api<Namespace> = kube::Api::all(client.clone());
    let project_namespace = ns_api.get(&name).await?;

    assert!(
        common::assert_is_owned_by_project(&project, &project_namespace).is_ok(),
        "namespace should be owned by project"
    );

    let wait_for_project_deleted_handle = common::wait_for_state(
        &kube::Api::<project::Project>::all(client.clone()),
        &name,
        WaitForState::Deleted,
    );

    let wait_for_namespace_deleted_handle =
        common::wait_for_state(&ns_api, &name, WaitForState::Deleted);

    assert!(
        kube::Api::<project::Project>::all(client.clone())
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

#[tokio::test]
#[serial]
async fn it_should_be_possible_to_update_projects() -> anyhow::Result<()> {
    let (client, operator) = common::before_each().await?;

    let name = common::random_name("update-project-test");
    let _ = common::install_project(&client, &name).await?;

    let project = Project::new(&name, Default::default());

    let result = operator.admission_hook(project).await;

    match result {
        AdmissionResult::Allow(_) => {}
        _ => panic!("admission hook should pass when a project gets updated"),
    }
    Ok(())
}

#[tokio::test]
#[serial]
async fn it_should_fail_if_namespace_already_exists_but_was_not_created_by_this_operator(
) -> anyhow::Result<()> {
    let (_, operator) = common::before_each().await?;

    let project = Project::new("default", Default::default());

    let result = operator.admission_hook(project).await;

    match result {
        AdmissionResult::Deny(status) => {
            assert_eq!(status.code, Some(409));
            assert_eq!(
                status.message,
                Some(
                    "can't create project: a namespace with name 'default' already exists"
                        .to_string()
                )
            );
            assert_eq!(status.status, Some("Failure".to_string()));
        }
        _ => panic!(
            "admission hook did not fail if a new project's name clashes with an existing namespace"
        ),
    }

    Ok(())
}

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
        Some(vec![(&project.kind).to_owned()]),
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
        subject.name, project.spec.owner,
        "cluster role binding subject name should be correct"
    );

    kube::Api::<project::Project>::all(client.clone())
        .delete(&name, &DeleteParams::default())
        .await?;

    Ok(())
}

#[tokio::test]
#[serial]
async fn it_fails_with_non_existant_owner_default_role_binding() -> anyhow::Result<()> {
    let kubeconfig = Kubeconfig::read_from(Path::new("./kind.kubeconfig"))?;
    let config =
        Config::from_custom_kubeconfig(kubeconfig, &config::KubeConfigOptions::default()).await?;

    let client = kube::Client::try_from(config.clone())?;
    match project::ProjectOperator::new(client.clone(), "non-existant-cluster-role-name", "default")
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

use noqnoqnoq::self_service::helper;

#[tokio::test]
#[serial]
async fn it_construct_a_correct_api_path_for_yaml_manifest() -> anyhow::Result<()> {
    let (client, _) = common::before_each().await?;
    // Create a pod from JSON
    let pod_manifest = include_str!("pod.yaml");
    let pod_api_path = helper::resource_path(&client, pod_manifest, "xxx").await?;
    assert_eq!("/api/v1/namespaces/xxx/pods".to_string(), pod_api_path);

    let deploy_manifest = include_str!("deployment.yaml");
    let deploy_api_path = helper::resource_path(&client, deploy_manifest, "xxx").await?;
    assert_eq!(
        "/apis/apps/v1/namespaces/xxx/deployments".to_string(),
        deploy_api_path
    );

    let role_manifest = include_str!("role.yaml");
    let role_api_path = helper::resource_path(&client, role_manifest, "xxx").await?;
    assert_eq!(
        "/apis/rbac.authorization.k8s.io/v1/namespaces/xxx/roles".to_string(),
        role_api_path
    );

    Ok(())
}

#[tokio::test]
#[serial]
async fn it_rejects_cluster_wide_manifests() -> anyhow::Result<()> {
    let (client, _) = common::before_each().await?;
    let self_service_project_manifest = include_str!("../self-service-project-manifest.yaml");
    let self_service_project_path =
        helper::resource_path(&client, self_service_project_manifest, "xxx").await;
    assert!(
        self_service_project_path.is_err(),
        "cluster non-namespaced resources should yield an error"
    );

    assert_eq!(
        self_service_project_path
            .err()
            .unwrap()
            .to_string()
            .as_str(),
        "only namespaced resources are supported: selfservice.innoq.io/v1/Project with name 'sample-self-service-project' is a cluster resource"
    );

    Ok(())
}

#[tokio::test]
#[serial]
async fn it_rejects_manifests_with_a_set_namespace() -> anyhow::Result<()> {
    let (client, _) = common::before_each().await?;
    // Create a pod from JSON
    let pod_manifest = include_str!("namespaced_pod.yaml");
    let pod_api_path = helper::resource_path(&client, pod_manifest, "xxx").await;
    assert!(
        pod_api_path.is_err(),
        "resources with explicit namespace should yield error"
    );

    assert_eq!(
        pod_api_path
            .err()
            .unwrap()
            .to_string()
            .as_str(),
        "setting namespace forbidden: resource v1/Pod with name 'foo' has namespace set explicitly to 'foo-namespace'"
    );

    Ok(())
}

#[tokio::test]
#[serial]
#[ignore = "not yet implemented"]
async fn it_should_not_create_namespace_if_a_resource_has_problems() -> anyhow::Result<()> {
    Ok(())
}

#[tokio::test]
#[serial]
async fn it_should_correctly_create_yaml_manifest_resources() -> anyhow::Result<()> {
    let (client, _) = common::before_each().await?;

    let name = common::random_name("apply-manifest");
    let project = common::install_project(&client, &name).await?;

    let sa_api = kube::Api::<ServiceAccount>::namespaced(client.clone(), &name);
    common::wait_for_state(&sa_api, &"default".to_string(), WaitForState::Created).await?;

    let default_sa = sa_api.get("default").await?;
    let default_secret_name = default_sa.secrets.as_ref().unwrap()[0]
        .name
        .as_ref()
        .unwrap();

    {
        let api = kube::Api::<Secret>::namespaced(client.clone(), &name);
        common::wait_for_state(&api, &default_secret_name, WaitForState::Created).await?;
    }

    // Create a pod from YAML
    let pod_manifest = include_str!("pod.yaml");

    helper::apply_yaml_manifest(&client, pod_manifest, &project).await?;

    let pod = kube::Api::<Pod>::namespaced(client.clone(), name.as_str())
        .get("foo")
        .await;

    assert!(
        &pod.is_ok(),
        "pod should have been created successfully: {}",
        pod.err().unwrap().to_string()
    );

    assert!(
        common::assert_is_owned_by_project(&project, &pod.unwrap()).is_ok(),
        "pod should be owned by project"
    );

    kube::Api::<project::Project>::all(client.clone())
        .delete(name.as_str(), &DeleteParams::default())
        .await?;

    Ok(())
}
