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

use k8s_openapi::api::core::v1::{ConfigMap, Pod, PodStatus};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, PostParams};
use kube::Resource;
use kube::ResourceExt;
use log::debug;
use serial_test::serial;
use tokio::select;
use tokio::time;

use self_service_operators::project::Sample;
use self_service_operators::project::{Project, ProjectSpec};

use crate::project;
use crate::project::{wait_for_state, WaitForState};
use self_service_operators::project::states::ProjectPhase;

#[tokio::test]
#[serial]
async fn it_should_eventually_install_correctly_rendered_manifests() -> anyhow::Result<()> {
    let (client, _) = project::before_each().await?;

    let name = project::random_name("install-manifests");
    let timeout_secs = 20;

    project::apply_manifest_secret(
        &client,
        "extra-manifests",
        vec![
            include_str!("../../fixtures/pod-sa.yaml"),
            include_str!("../../fixtures/sa.yaml"),
            include_str!("../../fixtures/config-map.yaml"),
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

    let wait_for_pod_created_handle = project::wait_for_state(
        &kube::Api::<Pod>::namespaced(client.clone(), &name),
        &"pod-sa".to_string(),
        WaitForState::Created,
    );

    assert!(
        select! {
        res = wait_for_pod_created_handle => res.is_ok(),
        _ = time::sleep(Duration::from_secs(timeout_secs)) => false
        },
        "namespace '{}' should contain a pod called 'pod-sa' after {} seconds",
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
        project::assert_project_is_in_phase(&client, &name, ProjectPhase::WaitingForChanges)
            .await
            .is_ok(),
        "project should be in waiting state"
    );

    Ok(())
}

#[tokio::test]
#[serial]
async fn it_should_not_recreate_apply_once_resources() -> anyhow::Result<()> {
    let (client, _) = project::before_each().await?;

    let name = project::random_name("apply-once-resources");
    let timeout_secs = 20;

    project::apply_manifest_secret(
        &client,
        "extra-manifests",
        vec![include_str!("../../fixtures/apply-once-resource.yaml")],
    )
    .await?;

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
        ..Default::default()
    };

    let api: kube::Api<Project> = kube::Api::all(client.clone());
    let _ = api.create(&PostParams::default(), &project).await;

    let wait_for_pod_created_handle = project::wait_for_state(
        &kube::Api::<Pod>::namespaced(client.clone(), &name),
        &"once".to_string(),
        WaitForState::Created,
    );

    assert!(
        select! {
        res = wait_for_pod_created_handle => res.is_ok(),
        _ = time::sleep(Duration::from_secs(timeout_secs)) => false
        },
        "namespace '{}' should contain a pod called 'once' after {} seconds",
        name,
        timeout_secs
    );

    let wait_for_pod_deleted_handle = project::wait_for_state(
        &kube::Api::<Pod>::namespaced(client.clone(), &name),
        &"once".to_string(),
        WaitForState::Deleted,
    );

    kube::Api::<Pod>::namespaced(client.clone(), &name)
        .delete("once", &DeleteParams::default())
        .await?;

    assert!(
        select! {
        res = wait_for_pod_deleted_handle => res.is_ok(),
        _ = time::sleep(Duration::from_secs(timeout_secs)) => false
        },
        "namespace '{}' should not contain a pod called 'once' anymore after {} seconds",
        name,
        timeout_secs
    );

    let wait_for_pod_created_handle = project::wait_for_state(
        &kube::Api::<Pod>::namespaced(client.clone(), &name),
        &"once".to_string(),
        WaitForState::Created,
    );

    assert!(
        select! {
        res = wait_for_pod_created_handle => res.is_err(),
        _ = time::sleep(Duration::from_secs(20)) => true
        },
        "pod 'once' should not be recreated",
    );

    Ok(())
}

#[tokio::test]
#[serial]
async fn it_should_error_with_invalid_manifests() -> anyhow::Result<()> {
    let (client, _) = project::before_each().await?;

    let name = project::random_name("error-on-failing-manifests");

    project::apply_manifest_secret(
        &client,
        "extra-manifests",
        vec![include_str!("../../fixtures/failing-manifest.yaml")],
    )
    .await?;

    let manifest_values = r#"
name: name_with_forbidden_underscores
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

    assert!(
        project::assert_project_is_in_phase(&client, &name, ProjectPhase::FailedDueToError)
            .await
            .is_ok(),
        "project should be in error state"
    );

    Ok(())
}

#[tokio::test]
#[serial]
async fn it_should_recover_after_project_was_fixed() -> anyhow::Result<()> {
    let (client, _) = project::before_each().await?;

    let name = project::random_name("recover-projects");

    project::apply_manifest_secret(
        &client,
        "extra-manifests",
        vec![include_str!("../../fixtures/failing-manifest.yaml")],
    )
    .await?;

    let manifest_values = r#"
name: name_with_forbidden_underscores
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
            annotations: Some(annotations.clone()),
            ..Default::default()
        },
        spec: spec.clone(),
        ..Default::default()
    };

    let api: kube::Api<Project> = kube::Api::all(client.clone());
    let _ = api.create(&PostParams::default(), &project).await?;

    assert!(
        project::assert_project_is_in_phase(&client, &name, ProjectPhase::FailedDueToError)
            .await
            .is_ok(),
        "project should be in error state"
    );

    let manifest_values = r#"
name: foooooo
"#;

    spec.manifest_values = Some(manifest_values.into());

    let mut project = api.get(&name).await?;
    let resource_version = project.resource_version();
    project.spec = spec;

    let meta = project.meta_mut();
    meta.resource_version = resource_version;
    meta.managed_fields = None;

    let _ = api
        .replace(&*name, &PostParams::default(), &project)
        .await?;

    assert!(
        project::assert_project_is_in_phase(&client, &name, ProjectPhase::WaitingForChanges)
            .await
            .is_ok(),
        "project should be in error state"
    );

    Ok(())
}

#[tokio::test]
#[serial]
async fn it_should_recreate_deleted_resources() -> anyhow::Result<()> {
    let (client, _) = project::before_each().await?;

    let name = project::random_name("recreate-deleted-resources");
    let _ = project::install_project(&client, &name).await?;

    let api: kube::Api<Project> = kube::Api::all(client.clone());

    let _ =
        project::assert_project_is_in_phase(&client, &name, ProjectPhase::WaitingForChanges).await;

    let pod_api = kube::Api::<Pod>::namespaced(client.clone(), &name);
    assert!(
        wait_for_state(&pod_api, &"foo".to_string(), WaitForState::Created)
            .await
            .is_ok()
    );

    loop {
        debug!("waiting for foo pod to be running");
        if let Ok(pod) = pod_api.get("foo").await {
            match pod.status {
                Some(PodStatus {
                    phase: Some(phase), ..
                }) if phase.as_str() == "Running" => break,
                _ => {}
            }
        }

        time::sleep(Duration::from_secs(1)).await
    }

    let _ = pod_api
        .delete(
            "foo",
            &DeleteParams {
                dry_run: false,
                grace_period_seconds: Some(0),
                ..Default::default()
            },
        )
        .await?;

    loop {
        debug!("waiting for foo pod to be gone");
        if pod_api.get("foo").await.is_err() {
            break;
        }
        time::sleep(Duration::from_secs(1)).await
    }

    let mut project = api.get(&name).await?;
    let resource_version = project.resource_version();
    project.spec = ProjectSpec {
        owners: vec!["newowner@example.com".to_string()],
        manifest_values: project.spec.manifest_values,
    };
    let meta = project.meta_mut();
    meta.resource_version = resource_version;
    meta.managed_fields = None;

    if let Err(e) = api.replace(&name, &PostParams::default(), &project).await {
        panic!("error updating project: {:?}:\n{:?}", &project, e);
    }

    assert!(
        project::assert_project_is_in_phase(&client, &name, ProjectPhase::WaitingForChanges)
            .await
            .is_ok(),
        "project should be in waiting state"
    );

    assert!(pod_api.get("foo").await.is_ok());

    Ok(())
}
