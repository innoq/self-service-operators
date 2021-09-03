use crate::common::WaitForState;
use k8s_openapi::api::core::v1::{Pod, Secret, ServiceAccount};
use k8s_openapi::ByteString;
use kube::api::DeleteParams;
use noqnoqnoq::project::{Project, ProjectSpec};
use noqnoqnoq::{helper, project};
use serial_test::serial;

mod common;

#[tokio::test]
#[serial]
async fn it_construct_a_correct_api_path_for_yaml_manifest() -> anyhow::Result<()> {
    let (client, _) = common::before_each().await?;

    let project = Project::new("xxx", ProjectSpec::default());

    // Create a pod from JSON
    let pod_manifest = project.render(
        &ByteString(include_str!("fixtures/pod.yaml").as_bytes().to_vec()),
        "foo",
    )?;

    let pod_api_path = helper::resource_path(&client, &pod_manifest).await?;
    assert_eq!("/api/v1/namespaces/xxx/pods/foo".to_string(), pod_api_path);

    let deploy_manifest = include_str!("fixtures/deployment.yaml");
    let deploy_api_path = helper::resource_path(&client, deploy_manifest).await?;
    assert_eq!(
        "/apis/apps/v1/namespaces/xxx/deployments/my-deployment".to_string(),
        deploy_api_path
    );

    let role_manifest = include_str!("fixtures/role.yaml");
    let role_api_path = helper::resource_path(&client, role_manifest).await?;
    assert_eq!(
        "/apis/rbac.authorization.k8s.io/v1/namespaces/xxx/roles/podreader".to_string(),
        role_api_path
    );

    Ok(())
}

#[tokio::test]
#[serial]
async fn it_rejects_manifests_with_an_unset_namespace() -> anyhow::Result<()> {
    let (client, _) = common::before_each().await?;
    // Create a pod from JSON
    let pod_manifest = include_str!("fixtures/missing-namespace-pod.yaml");
    let pod_api_path = helper::resource_path(&client, pod_manifest).await;
    assert!(
        pod_api_path.is_err(),
        "resources with missing namespace should yield error"
    );

    assert_eq!(
        pod_api_path
            .err()
            .unwrap()
            .to_string()
            .as_str(),
        "setting namespace is required: resource v1/Pod with name 'foo' has no namespace set ... in most cases you want to set it to {{ __PROJECT_NAME__ }}\nManifest is: ---\napiVersion: v1\nkind: Pod\nmetadata:\n  name: foo\nspec:\n  containers:\n    - name: foo\n      image: alpine\n      command: ['sh', '-c', 'echo Hello Kubernetes! && sleep 3600']\n",
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
    let pod_manifest = include_str!("fixtures/pod2.yaml");
    let templated_manifest = project.render(
        &k8s_openapi::ByteString(pod_manifest.as_bytes().to_vec()),
        "foo",
    );
    helper::apply_yaml_manifest(&client, &templated_manifest.unwrap(), &project).await?;

    let pod = kube::Api::<Pod>::namespaced(client.clone(), name.as_str())
        .get("bar")
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
