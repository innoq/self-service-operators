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
        Duration::from_secs(0)
	)
	.await
	{
		Ok(_) => panic!(
			"project operator should fail if the given default manifests secret does not exist"
		),
		Err(e) => assert_eq!(
			e.to_string(),
			"no Secret with name 'non-existant-secret' in namespace 'default' found (this secret should hold default manifests that get applied in each new namespace): ApiError: secrets \"non-existant-secret\" not found: NotFound (ErrorResponse { status: \"Failure\", message: \"secrets \\\"non-existant-secret\\\" not found\", reason: \"NotFound\", code: 404 }) -- aborting",
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
async fn it_should_correctly_copy_and_template_default_manifests() -> anyhow::Result<()> {
    let (client, _operator) = common::before_each().await?;
    common::apply_manifest_secret(
        &client,
        project::DEFAULT_MANIFESTS_SECRET,
        vec![include_str!("fixtures/templated-pod.yaml")],
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
async fn it_should_correctly_copy_annotated_manifests() -> anyhow::Result<()> {
    let (client, _operator) = common::before_each().await?;
    common::apply_manifest_secret(
        &client,
        "extra-manifests",
        vec![include_str!("fixtures/templated-pod.yaml")],
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
        "project.selfservice.innoq.io/extra-manifests.resource0".to_string(),
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
async fn it_should_skip_annotated_manifests() -> anyhow::Result<()> {
    let (client, _operator) = common::before_each().await?;
    common::apply_manifest_secret(
        &client,
        "extra-manifests",
        vec![include_str!("fixtures/templated-pod.yaml")],
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
        "project.selfservice.innoq.io/extra-manifests.resource0".to_string(),
        "skip".to_string(),
    );
    annotations.insert(
        "project.selfservice.innoq.io/extra-manifests".to_string(),
        "copy".to_string(),
    );

    annotations.insert(
        "project.selfservice.innoq.io/default-project-manifests.resource0".to_string(),
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

    let manifests = project
        .associated_manifests(&client, project::DEFAULT_MANIFESTS_SECRET, "default")
        .await?;

    // println!("{}", manifests[0]);
    assert_eq!(manifests.len(), 0, "all manifests should be skipped");

    Ok(())
}

#[tokio::test]
#[serial]
async fn it_should_eventually_install_manifests() -> anyhow::Result<()> {
    let (client, _) = common::before_each().await?;

    let name = common::random_name("install-manifests");
    let timeout_secs = 20;

    common::apply_manifest_secret(
        &client,
        "extra-manifests",
        vec![
            include_str!("fixtures/pod-sa.yaml"),
            include_str!("fixtures/sa.yaml"),
        ],
    )
    .await?;

    let mut manifest_values = HashMap::new();
    manifest_values.insert("name".to_string(), "standard-pod".to_string());

    let mut spec = project::ProjectSpec::sample();
    spec.manifest_values = Some(manifest_values);

    let mut annotations = BTreeMap::new();
    annotations.insert(
        "project.selfservice.innoq.io/extra-manifests".to_string(),
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

    let project_api: kube::Api<project::Project> = kube::Api::all(client.clone());
    let _ = project_api.create(&PostParams::default(), &project).await;

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

    assert!(
        common::assert_project_is_in_waiting_state(&client, &name)
            .await
            .is_ok(),
        "project should be in waiting state"
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
async fn it_fails_with_correct_error_state_when_invalid_manifests_are_used() -> anyhow::Result<()> {
    let (client, _) = common::before_each().await?;

    let name = common::random_name("fail-with-invalid-manifests");

    common::apply_manifest_secret(
        &client,
        "extra-manifests",
        vec![include_str!("fixtures/invalid.yaml")],
    )
    .await?;

    let mut manifest_values = HashMap::new();
    manifest_values.insert("name".to_string(), "standard-pod".to_string());

    let mut spec = project::ProjectSpec::sample();
    spec.manifest_values = Some(manifest_values);

    let mut annotations = BTreeMap::new();
    annotations.insert(
        "project.selfservice.innoq.io/extra-manifests".to_string(),
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

    let api: kube::Api<project::Project> = kube::Api::all(client.clone());
    let _ = api.create(&PostParams::default(), &project).await?;

    let mut last_summary = "".to_string();
    for _ in 0..20 {
        let _ = time::sleep(Duration::from_secs(1)).await;
        let project = api.get(&name).await?;
        if project.status.clone().unwrap().summary.is_none() {
            continue;
        }

        let status = &project.status.clone().unwrap();
        last_summary = status.summary.clone().unwrap();
        if last_summary == *"error: error installing manifest: giving up aft..." {
            return Ok(());
        }
    }

    panic!(
        "correct error state never reached -- last error summary was: {}",
        last_summary
    );
}
