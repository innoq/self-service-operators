/*
 * Copyright 2021 Daniel Bornkessel
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use k8s_openapi::api::core::v1::{Pod, Secret, ServiceAccount};
use kube::api::DeleteParams;
use serial_test::serial;

use self_service_operators::project::states::apply_manifests;
use self_service_operators::project::Project;
use self_service_operators::project::ProjectSpec;

use crate::project;
use crate::project::WaitForState;

#[tokio::test]
#[serial]
async fn it_construct_a_correct_api_path_for_yaml_manifest() -> anyhow::Result<()> {
    let (client, _) = project::before_each().await?;

    let project = Project::new("xxx", ProjectSpec::default());

    // Create a pod from JSON
    let pod_manifest = project.render(include_str!("../fixtures/pod.yaml"), "foo")?;

    let pod_api_path = apply_manifests::resource_path(&client, &pod_manifest).await?;
    assert_eq!("/api/v1/namespaces/xxx/pods/foo".to_string(), pod_api_path);

    let deploy_manifest = include_str!("../fixtures/deployment.yaml");
    let deploy_api_path = apply_manifests::resource_path(&client, deploy_manifest).await?;
    assert_eq!(
        "/apis/apps/v1/namespaces/xxx/deployments/my-deployment".to_string(),
        deploy_api_path
    );

    let role_manifest = include_str!("../fixtures/role.yaml");
    let role_api_path = apply_manifests::resource_path(&client, role_manifest).await?;
    assert_eq!(
        "/apis/rbac.authorization.k8s.io/v1/namespaces/xxx/roles/podreader".to_string(),
        role_api_path
    );

    Ok(())
}

#[tokio::test]
#[serial]
async fn it_rejects_manifests_with_an_unset_namespace() -> anyhow::Result<()> {
    let (client, _) = project::before_each().await?;
    // Create a pod from JSON
    let pod_manifest = include_str!("../fixtures/missing-namespace-pod.yaml");
    let pod_api_path = apply_manifests::resource_path(&client, pod_manifest).await;
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
    let (client, _) = project::before_each().await?;

    let name = project::random_name("apply-manifest");
    let project = project::install_project(&client, &name).await?;

    let api = kube::Api::<ServiceAccount>::namespaced(client.clone(), &name);
    project::wait_for_state(&api, &"default".to_string(), WaitForState::Created).await?;

    let default_sa = api.get("default").await?;
    let default_secret_name = default_sa.secrets.as_ref().unwrap()[0]
        .name
        .as_ref()
        .unwrap();

    {
        let api = kube::Api::<Secret>::namespaced(client.clone(), &name);
        project::wait_for_state(&api, &default_secret_name, WaitForState::Created).await?;
    }

    // Create a pod from YAML
    let pod_manifest = include_str!("../fixtures/pod2.yaml");
    let templated_manifest = project.render(&pod_manifest, "foo");
    apply_manifests::apply_yaml_manifest(&client, &templated_manifest.unwrap(), &project).await?;

    let pod = kube::Api::<Pod>::namespaced(client.clone(), name.as_str())
        .get("bar")
        .await;

    assert!(
        &pod.is_ok(),
        "pod should have been created successfully: {}",
        pod.err().unwrap().to_string()
    );

    assert!(
        project::assert_is_owned_by_project(&project, &pod.unwrap()).is_ok(),
        "pod should be owned by project"
    );

    kube::Api::<Project>::all(client.clone())
        .delete(name.as_str(), &DeleteParams::default())
        .await?;

    Ok(())
}
