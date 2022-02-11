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




use std::time::{Duration};
use tokio::{select, time};



use k8s_openapi::api::core::v1::{Namespace};


use krator::OperatorRuntime;
use kube::api::{PostParams};
use kube::{Client};


use self_service_operators::project::operator::ProjectOperator;
use self_service_operators::project::project::{
    DEFAULT_MANIFESTS_SECRET,
};
use self_service_operators::project::{ProjectSpec, Sample};

use crate::WaitForState;
use self_service_operators::project::states::ProjectPhase;
use self_service_operators::project::Project;

mod admission_webhook_tests;
mod manifest_secrets;
mod operator;
mod project;
mod states;
mod yaml_manifest_parsing;

pub async fn before_each() -> anyhow::Result<(kube::Client, ProjectOperator)> {
    let (config, client) = crate::get_client().await?;

    assert!(
        crate::apply_manifest_secret(
            &client,
            DEFAULT_MANIFESTS_SECRET,
            vec![
                include_str!("../fixtures/pod.yaml"),
                include_str!("../../manifests/default-project-manifests/owner-clusterrole.yaml"),
                include_str!(
                    "../../manifests/default-project-manifests/owner-clusterrolebinding.yaml"
                ),
                include_str!(
                    "../../manifests/default-project-manifests/project-owner-role-binding.yaml"
                ),
            ]
        )
        .await
        .is_ok(),
        "installing default manifest secret should work"
    );

    // there is probably a better way FnOnce?
    let _ = crate::reinstall_self_service_crd(
        &client,
        &Project::crd(),
        Project::admission_webhook_resources("default"),
    )
    .await?;

    let operator = self_service_operators::project::operator::ProjectOperator::new(
        client.clone(),
        "default",
        DEFAULT_MANIFESTS_SECRET,
        Duration::from_secs(0),
    )
    .await
    .unwrap();
    let mut runtime = OperatorRuntime::new(&config, operator.clone(), None);

    tokio::spawn(async move { runtime.start().await });

    Ok((client, operator))
}

pub async fn install_project(
    client: &kube::Client,
    #[allow(clippy::ptr_arg)] name: &String,
) -> anyhow::Result<Project> {
    let _ = env_logger::builder()
        // Ensure events are captured by `cargo test`
        .is_test(true)
        // Ignore errors initializing the logger if tests race to configure it
        .try_init();

    let timeout_secs = 20;

    let wait_for_namespace_created_handle = crate::wait_for_state(
        &kube::Api::<Namespace>::all(client.clone()),
        name,
        WaitForState::Created,
    );

    let project_api: kube::Api<Project> = kube::Api::all(client.clone());
    let wait_for_project_created_handle =
        crate::wait_for_state(&project_api, name, WaitForState::Created);

    let manifest_values = "name: templated-name";

    let mut spec = ProjectSpec::sample();
    spec.manifest_values = Some(manifest_values.into());

    let project = Project::new(name.as_str(), spec);

    let project_resource = project_api.create(&PostParams::default(), &project).await;

    assert!(
        project_resource.is_ok(),
        "creating a new self service project should work correctly: {}",
        project_resource.err().unwrap()
    );
    assert!(
        select! {
        res = futures::future::try_join(wait_for_namespace_created_handle, wait_for_project_created_handle) => res.is_ok(),
            _ = time::sleep(Duration::from_secs(timeout_secs)) => false,
        },
        "expected project related namespace {} to be created within {} seconds",
        name,
        timeout_secs
    );

    Ok(project_resource.unwrap())
}

#[allow(dead_code)] // it's not used by every test and therefore sometimes throws warnings
pub fn assert_is_owned_by_project<R>(project: &Project, resource: &R) -> anyhow::Result<()>
where
    R: kube::Resource + k8s_openapi::Resource,
{
    assert!(
        resource.meta().owner_references.is_some(),
        "{} should have owner reference",
        R::KIND
    );

    let owners = resource.meta().owner_references.as_ref().unwrap();
    assert!(
        !owners.is_empty(),
        "{} should have at least one owner",
        R::KIND
    );

    let owner = &owners[0];
    assert_eq!(
        owner.api_version, project.api_version,
        "api_version of owner-reference is wrong"
    );
    assert_eq!(
        owner.controller,
        Some(true),
        "controller of owner-reference is wrong"
    );
    assert_eq!(owner.kind, project.kind, "kind of owner-reference is wrong");
    assert_eq!(
        owner.name,
        project.metadata.name.clone().unwrap(),
        "name of owner-reference is wrong"
    );
    assert_eq!(
        owner.uid,
        project.metadata.uid.clone().unwrap(),
        "uid of owner-reference is wrong: owner: {:#?}, project: {:#?}",
        owner,
        project
    );

    Ok(())
}

#[allow(dead_code)] // it's not used by every test and therefore sometimes throws warnings
pub async fn assert_project_is_in_phase(
    client: &Client,
    name: &str,
    phase: ProjectPhase,
) -> anyhow::Result<()> {
    let mut current_project_phase = None;

    let api: kube::Api<Project> = kube::Api::all(client.clone());
    for _ in 0..10 {
        let _ = tokio::time::sleep(Duration::from_secs(1)).await;
        let project = api.get(name).await?;

        if let Some(status) = &project.status.clone() {
            current_project_phase = status.phase.clone();

            if current_project_phase.is_some()
                && std::mem::discriminant(current_project_phase.as_ref().unwrap())
                    == std::mem::discriminant(&phase)
            {
                return Ok(());
            }
        }
    }

    panic!(
        "project should be in phase {:?} but is in phase {:?}",
        phase, current_project_phase
    );
}
