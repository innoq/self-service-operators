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

use core::time::Duration;
use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::{Namespace, Pod, Secret};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::PostParams;
use serial_test::serial;
use tokio::select;
use tokio::time;

use self_service_operators::project::project::{
    DEFAULT_MANIFESTS_SECRET, SECRET_ANNOTATION_KEY, SECRET_ANNOTATION_VALUE,
};
use self_service_operators::project::{operator, Sample};
use self_service_operators::project::{Project, ProjectSpec};

use crate::project;
use crate::project::WaitForState;
use self_service_operators::project::states::ProjectPhase;

#[tokio::test]
#[serial]
async fn it_should_only_copy_from_annotated_secrets() -> anyhow::Result<()> {
    let (client, _) = project::before_each().await?;

    let name = project::random_name("secret-annotations");
    let _ = project::install_project(&client, &name).await?;

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
        SECRET_ANNOTATION_KEY.to_string(),
        SECRET_ANNOTATION_VALUE.to_string(),
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

    let resources = operator::get_manifests_secret(&client, "i-do-not-exist", &name).await;
    assert!(resources.is_err());
    assert_eq!(resources.unwrap_err().to_string(), "ApiError: secrets \"i-do-not-exist\" not found: NotFound (ErrorResponse { status: \"Failure\", message: \"secrets \\\"i-do-not-exist\\\" not found\", reason: \"NotFound\", code: 404 })");

    let resources = operator::get_manifests_secret(&client, "standard-secret", &name).await;
    assert!(resources.is_err());
    assert_eq!(
		resources.unwrap_err().to_string(),
		format!(
            "Error accessing secret 'standard-secret': only secrets with the annotation '{}: {}' can be accessed by the project operator",
            SECRET_ANNOTATION_KEY,
            SECRET_ANNOTATION_VALUE
		)
	);

    let resources = operator::get_manifests_secret(&client, "annotated-secret", &name).await;
    assert!(resources.is_ok());

    assert!(
        project::assert_project_is_in_phase(&client, &name, ProjectPhase::WaitingForChanges)
            .await
            .is_ok(),
        "project should be in waiting state"
    );

    Ok(())
}

#[tokio::test]
#[serial]
async fn it_should_correctly_copy_default_manifests() -> anyhow::Result<()> {
    let (client, _) = project::before_each().await?;

    let name = project::random_name("copy-default-secrets-manifests");
    let timeout_secs = 20;

    let wait_for_pod_created_handle = project::wait_for_state(
        &kube::Api::<Pod>::namespaced(client.clone(), &name),
        &"foo".to_string(),
        WaitForState::Created,
    );

    let _ = project::install_project(&client, &name).await?;
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
        project::assert_project_is_in_phase(&client, &name, ProjectPhase::WaitingForChanges)
            .await
            .is_ok(),
        "project should be in waiting state"
    );

    Ok(())
}

#[tokio::test]
#[serial]
async fn it_should_correctly_copy_and_template_default_manifests() -> anyhow::Result<()> {
    let (client, _operator) = project::before_each().await?;
    project::apply_manifest_secret(
        &client,
        DEFAULT_MANIFESTS_SECRET,
        vec![include_str!("../fixtures/templated-pod.yaml")],
    )
    .await?;

    let name = project::random_name("copy-and-template-default-secrets-manifests");
    let timeout_secs = 20;

    let wait_for_pod_created_handle = project::wait_for_state(
        &kube::Api::<Pod>::namespaced(client.clone(), &name),
        &"templated-name".to_string(),
        WaitForState::Created,
    );

    let _ = project::install_project(&client, &name).await?;
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
        project::assert_project_is_in_phase(&client, &name, ProjectPhase::WaitingForChanges)
            .await
            .is_ok(),
        "project should be in waiting state"
    );

    Ok(())
}

#[tokio::test]
#[serial]
async fn it_should_correctly_copy_annotated_manifests() -> anyhow::Result<()> {
    let (client, _operator) = project::before_each().await?;
    project::apply_manifest_secret(
        &client,
        "extra-manifests",
        vec![include_str!("../fixtures/templated-pod.yaml")],
    )
    .await?;

    let name = project::random_name("copy-annotated-manifest");
    let timeout_secs = 20;

    let manifest_values = "name: extra-pod";
    let mut spec = ProjectSpec::sample();
    spec.manifest_values = Some(manifest_values.into());

    let mut annotations = BTreeMap::new();
    annotations.insert(
        "project.selfservice.innoq.io/extra-manifests.resource0".to_string(),
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

    let _ = kube::Api::all(client.clone())
        .create(&PostParams::default(), &project)
        .await?;

    project::wait_for_state(
        &kube::Api::<Namespace>::all(client.clone()),
        &name,
        WaitForState::Created,
    )
    .await?;

    let wait_for_pod_created_handle = project::wait_for_state(
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
        project::assert_project_is_in_phase(&client, &name, ProjectPhase::WaitingForChanges)
            .await
            .is_ok(),
        "project should be in waiting state"
    );

    Ok(())
}

#[tokio::test]
#[serial]
async fn it_should_skip_annotated_manifests() -> anyhow::Result<()> {
    let (client, _operator) = project::before_each().await?;
    project::apply_manifest_secret(
        &client,
        "extra-manifests",
        vec![include_str!("../fixtures/templated-pod.yaml")],
    )
    .await?;

    let name = project::random_name("skip-annotated-manifest");

    let manifest_values = "- name: extra-pod";

    let mut spec = ProjectSpec::sample();
    spec.manifest_values = Some(manifest_values.into());

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
        "project.selfservice.innoq.io/default-project-manifests".to_string(),
        "skip".to_string(),
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

    let manifests = project
        .associated_manifests(&client, DEFAULT_MANIFESTS_SECRET, "default")
        .await?;

    // println!("{}", manifests[0]);
    assert_eq!(manifests.len(), 0, "all manifests should be skipped");

    Ok(())
}
