use crate::common::WaitForState;
use k8s_openapi::api::core::v1::{Pod, Secret, ServiceAccount};
use kube::api::DeleteParams;
use noqnoqnoq::{helper, project};
use serial_test::serial;

mod common;

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
