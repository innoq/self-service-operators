mod common;

use crate::common::WaitForState;
use k8s_openapi::api::core::v1::{Namespace, Pod, Secret};
use kube::api::{ObjectMeta, PostParams};
use noqnoqnoq::{
    helper,
    project::{self},
    self_service::Sample,
};
use serial_test::serial;
use std::collections::{BTreeMap, HashMap};
use std::time::Duration;
use tokio::select;
use tokio::time;

#[tokio::test]
#[serial]
async fn it_fails_with_non_existant_default_manifests_secret() -> anyhow::Result<()> {
    let (_, client) = common::get_client().await?;

    match project::ProjectOperator::new(
        client.clone(),
        project::OWNER_ROLE_BINDING_NAME,
        "default",
        "non-existant-secret",
    )
    .await
    {
        Ok(_) => panic!(
            "project operator should fail if the given default manifests secret does not exist"
        ),
        Err(e) => assert_eq!(
            e.to_string(),
            "no Secret with name 'non-existant-secret' in namespace 'default' found (this secret should hold default manifests that get applied in each new namespace) -- aborting",
            "error message should be correct"
        ),
    };
    Ok(())
}

#[tokio::test]
#[serial]
async fn it_should_only_copy_from_annotated_secrets() -> anyhow::Result<()> {
    let (client, _) = common::before_each().await?;

    let name = common::random_name("secret-annotations");
    let _ = common::install_project(&client, &name).await?;

    let api = kube::Api::<Secret>::namespaced(client.clone(), &name);

    let _standard_secret = api
        .create(
            &PostParams::default(),
            &Secret {
                data: Some(BTreeMap::new()),
                metadata: ObjectMeta {
                    name: Some("standard-secret".to_string()),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .await?;

    let mut annotations = BTreeMap::new();
    annotations.insert(
        project::SECRET_ANNOTATION_KEY.to_string(),
        project::SECRET_ANNOTATION_VALUE.to_string(),
    );
    let _annotated_secret = api
        .create(
            &PostParams::default(),
            &Secret {
                data: Some(BTreeMap::new()),
                metadata: ObjectMeta {
                    annotations: Some(annotations),
                    name: Some("annotated-secret".to_string()),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .await?;

    let resources = helper::get_manifests_secret(&client, "i-do-not-exist", &name).await;
    assert!(resources.is_err());
    assert_eq!(resources.unwrap_err().to_string(), "ApiError: secrets \"i-do-not-exist\" not found: NotFound (ErrorResponse { status: \"Failure\", message: \"secrets \\\"i-do-not-exist\\\" not found\", reason: \"NotFound\", code: 404 })");

    let resources = helper::get_manifests_secret(&client, "standard-secret", &name).await;
    assert!(resources.is_err());
    assert_eq!(
        resources.unwrap_err().to_string(),
        format!(
            "Error accessing secret 'standard-secret': only secrets with the annotation '{}: {}' can be accessed by the project operator",
            project::SECRET_ANNOTATION_KEY,
            project::SECRET_ANNOTATION_VALUE
        )
    );

    let resources = helper::get_manifests_secret(&client, "annotated-secret", &name).await;
    assert!(resources.is_ok());

    Ok(())
}

#[tokio::test]
#[serial]
async fn it_should_correctly_copy_default_manifests() -> anyhow::Result<()> {
    let (client, _) = common::before_each().await?;

    let name = common::random_name("copy-default-secrets-manifests");
    let timeout_secs = 20;

    let wait_for_pod_created_handle = common::wait_for_state(
        &kube::Api::<Pod>::namespaced(client.clone(), &name),
        &"foo".to_string(),
        WaitForState::Created,
    );

    let _ = common::install_project(&client, &name).await?;
    assert!(
        select! {
        res = wait_for_pod_created_handle => res.is_ok(),
        _ = time::sleep(Duration::from_secs(timeout_secs)) => false
        },
        "namespace '{}' should contain a pod called 'foo' should be present after {} seconds",
        name,
        timeout_secs
    );

    Ok(())
}

#[tokio::test]
#[serial]
async fn it_should_correctly_copy_and_template_default_manifests() -> anyhow::Result<()> {
    let (client, _operator) = common::before_each().await?;
    common::apply_manifest_secret(
        &client,
        project::DEFAULT_MANIFESTS_SECRET,
        include_str!("templated-pod.yaml"),
    )
    .await?;

    let name = common::random_name("copy-and-template-default-secrets-manifests");
    let timeout_secs = 20;

    let wait_for_pod_created_handle = common::wait_for_state(
        &kube::Api::<Pod>::namespaced(client.clone(), &name),
        &"templated-name".to_string(),
        WaitForState::Created,
    );

    let _ = common::install_project(&client, &name).await?;
    assert!(
        select! {
        res = wait_for_pod_created_handle => res.is_ok(),
        _ = time::sleep(Duration::from_secs(timeout_secs)) => false
        },
        "namespace '{}' should contain a pod called 'templated-name' should be present after {} seconds",
        name,
        timeout_secs
    );

    Ok(())
}

#[tokio::test]
#[serial]
async fn it_should_correctly_copy_annotated_manifests() -> anyhow::Result<()> {
    let (client, _operator) = common::before_each().await?;
    common::apply_manifest_secret(
        &client,
        "extra-manifests",
        include_str!("templated-pod.yaml"),
    )
    .await?;

    let name = common::random_name("copy-annotated-manifest");
    let timeout_secs = 20;

    let mut manifest_values = HashMap::new();
    manifest_values.insert("name".to_string(), "extra-pod".to_string());

    let mut spec = project::ProjectSpec::sample();
    spec.manifest_values = Some(manifest_values);

    let mut annotations = BTreeMap::new();
    annotations.insert(
        "project.selfservice.innoq.io/extra-manifests.pod.yaml".to_string(),
        "copy".to_string(),
    );

    let project = project::Project {
        metadata: ObjectMeta {
            name: Some(name.clone()),
            annotations: Some(annotations),
            ..Default::default()
        },
        spec,
        ..Default::default()
    };

    let _ = kube::Api::all(client.clone())
        .create(&PostParams::default(), &project)
        .await?;

    common::wait_for_state(
        &kube::Api::<Namespace>::all(client.clone()),
        &name,
        WaitForState::Created,
    )
    .await?;

    let wait_for_pod_created_handle = common::wait_for_state(
        &kube::Api::<Pod>::namespaced(client.clone(), &name),
        &"extra-pod".to_string(),
        WaitForState::Created,
    );

    assert!(
        select! {
        res = wait_for_pod_created_handle => res.is_ok(),
        _ = time::sleep(Duration::from_secs(timeout_secs)) => false
        },
        "namespace '{}' should contain a pod called 'extra-pod' should be present after {} seconds",
        name,
        timeout_secs
    );

    Ok(())
}

#[tokio::test]
#[serial]
async fn it_should_skip_annotated_manifests() -> anyhow::Result<()> {
    let (client, _operator) = common::before_each().await?;
    common::apply_manifest_secret(
        &client,
        "extra-manifests",
        include_str!("templated-pod.yaml"),
    )
    .await?;

    let name = common::random_name("skip-annotated-manifest");

    let mut manifest_values = HashMap::new();
    manifest_values.insert("name".to_string(), "extra-pod".to_string());

    let mut spec = project::ProjectSpec::sample();
    spec.manifest_values = Some(manifest_values);

    let mut annotations = BTreeMap::new();

    // copy all manifests from extra-manifests but skip the 'pod.yaml' manifest
    annotations.insert(
        "project.selfservice.innoq.io/extra-manifests.pod.yaml".to_string(),
        "skip".to_string(),
    );
    annotations.insert(
        "project.selfservice.innoq.io/extra-manifests".to_string(),
        "copy".to_string(),
    );

    annotations.insert(
        "project.selfservice.innoq.io/default-project-manifests.pod.yaml".to_string(),
        "skip".to_string(),
    );

    let project = project::Project {
        metadata: ObjectMeta {
            name: Some(name.clone()),
            annotations: Some(annotations),
            ..Default::default()
        },
        spec,
        ..Default::default()
    };

    let manifests = project.associated_manifests(&client, "default").await?;

    // println!("{}", manifests[0]);
    assert_eq!(manifests.len(), 0, "all manifests should be skipped");

    Ok(())
}
