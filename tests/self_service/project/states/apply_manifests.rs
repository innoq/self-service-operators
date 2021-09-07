use core::time::Duration;
use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::{ConfigMap, Pod};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::PostParams;
use serial_test::serial;
use tokio::select;
use tokio::time;

use self_service_operators::self_service::project::Sample;
use self_service_operators::self_service::project::{Project, ProjectSpec};

use crate::common;
use crate::common::WaitForState;

#[tokio::test]
#[serial]
async fn it_should_eventually_install_correctly_rendered_manifests() -> anyhow::Result<()> {
    let (client, _) = common::before_each().await?;

    let name = common::random_name("install-manifests");
    let timeout_secs = 20;

    common::apply_manifest_secret(
        &client,
        "extra-manifests",
        vec![
            include_str!("../../../fixtures/pod-sa.yaml"),
            include_str!("../../../fixtures/sa.yaml"),
            include_str!("../../../fixtures/config-map.yaml"),
        ],
    )
    .await?;

    let manifest_values = r#"
name: standard-pod
foo:
    bar:
        baz: boom!
array:
    - one
    - two
    - three
"#;

    let mut spec = ProjectSpec::sample();
    spec.manifest_values = Some(manifest_values.into());

    let mut annotations = BTreeMap::new();
    annotations.insert(
        "project.selfservice.innoq.io/extra-manifests".to_string(),
        "copy".to_string(),
    );

    let project = Project {
        metadata: ObjectMeta {
            name: Some(name.clone()),
            annotations: Some(annotations),
            ..Default::default()
        },
        spec,
        ..Default::default()
    };

    let api: kube::Api<Project> = kube::Api::all(client.clone());
    let _ = api.create(&PostParams::default(), &project).await;

    let wait_for_pod_created_handle = common::wait_for_state(
        &kube::Api::<Pod>::namespaced(client.clone(), &name),
        &"pod-sa".to_string(),
        WaitForState::Created,
    );

    assert!(
        select! {
        res = wait_for_pod_created_handle => res.is_ok(),
        _ = time::sleep(Duration::from_secs(timeout_secs)) => false
        },
        "namespace '{}' should contain a pod called 'pod-sa' should be present after {} seconds",
        name,
        timeout_secs
    );

    let api: kube::Api<ConfigMap> = kube::Api::namespaced(client.clone(), &name);
    let cm = api.get("test").await?;

    assert!(cm.data.is_some(), "config map should contain data");
    assert_eq!(
        cm.data.as_ref().unwrap().len(),
        5,
        "config map should three data items"
    );

    assert_eq!(
        cm.data.as_ref().unwrap().get("fooBarBaz"),
        Some(&"boom!".to_string()),
        "mapped values should be correctly rendered"
    );
    assert_eq!(
        cm.data.as_ref().unwrap().get("arrayZero"),
        Some(&"one".to_string()),
        "mapped values should be correctly rendered"
    );
    assert_eq!(
        cm.data.as_ref().unwrap().get("arrayTwo"),
        Some(&"three".to_string()),
        "mapped values should be correctly rendered"
    );
    assert_eq!(
        cm.data.as_ref().unwrap().get("name"),
        Some(&name),
        "mapped values should be correctly rendered"
    );
    assert_eq!(
        cm.data.as_ref().unwrap().get("owners"),
        Some(&"superdev@example.com supradev@example.com".to_string()),
        "mapped values should be correctly rendered"
    );

    assert!(
        common::assert_project_is_in_waiting_state(&client, &name)
            .await
            .is_ok(),
        "project should be in waiting state"
    );

    Ok(())
}
