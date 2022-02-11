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

// Note: there used to be a state called SeuptRBACPermissions. Keeping these these
// tests to validate the provided default manifest secrets

use core::time::Duration;

use k8s_openapi::api::rbac::v1::{ClusterRole, ClusterRoleBinding, RoleBinding};
use kube::Resource;
use serial_test::serial;
use tokio::select;
use tokio::time;

use self_service_operators::project::Project;

use crate::project;
use crate::WaitForState;
use self_service_operators::project::states::ProjectPhase;

#[tokio::test]
#[serial]
async fn it_should_create_clusterrole_and_clusterrolebinding_for_handling_this_project(
) -> anyhow::Result<()> {
    let (client, _) = project::before_each().await?;
    let timeout_secs = 60;
    let name = crate::random_name("owner-cluster-role-test");

    let project = project::install_project(&client, &name).await?;

    let cr_api = kube::Api::<ClusterRole>::all(client.clone());
    let crb_api = kube::Api::<ClusterRoleBinding>::all(client.clone());

    let resource_name = format!("selfservice:project:owner:{}", name);

    let wait_for_clusterrole_created_handle =
        crate::wait_for_state(&cr_api, &resource_name, WaitForState::Created);

    let wait_for_clusterrolebinding_created_handle =
        crate::wait_for_state(&crb_api, &resource_name, WaitForState::Created);

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

    let cr = cr_api.get(&resource_name).await?;
    assert!(
        project::assert_is_owned_by_project(&project, &cr).is_ok(),
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
        Some(vec!["projects".to_string()]),
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

    let crb = crb_api.get(&*resource_name).await?;
    assert!(
        project::assert_is_owned_by_project(&project, &crb).is_ok(),
        "owner cluster role binding should be owned by project"
    );

    assert_eq!(
        crb.role_ref.kind, "ClusterRole",
        "owner rolebinding role-ref kind should be ClusterRole"
    );

    assert_eq!(
        crb.role_ref.name, resource_name,
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
        subject.name, project.spec.owners[0],
        "cluster role binding subject name should be correct"
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
async fn it_creates_rolebinding() -> anyhow::Result<()> {
    let (client, _) = project::before_each().await?;
    let timeout_secs = 10;
    let name = crate::random_name("rolebinding-test");

    let project = project::install_project(&client, &name).await?;
    let resource_name = "selfservice:project:owner";

    let api = kube::Api::<RoleBinding>::namespaced(client.clone(), name.as_str());
    let wait_for_rolebinding_created_handle =
        crate::wait_for_state(&api, &resource_name.to_string(), WaitForState::Created);

    assert!(
        select! {
        res = wait_for_rolebinding_created_handle => res.is_ok(),
        _ = time::sleep(Duration::from_secs(timeout_secs)) => false
        },
        "rolebinding for the owner should be created within {} seconds",
        timeout_secs
    );

    let rb = api.get(resource_name).await?;

    assert!(
        project::assert_is_owned_by_project(&project, &rb).is_ok(),
        "owner role binding should be owned by project"
    );

    assert_eq!(
        rb.metadata.name.unwrap(),
        resource_name,
        "owner rolebinding name should be correct"
    );
    assert!(
        rb.subjects.is_some() && !rb.subjects.as_ref().unwrap().is_empty(),
        "owner rolebinding subject should exit"
    );

    let subject = &rb.subjects.as_ref().unwrap()[0];
    assert_eq!(
        subject.name, project.spec.owners[0],
        "subject name should be correct"
    );

    let subject1 = &rb.subjects.as_ref().unwrap()[1];
    assert_eq!(
        subject1.name, project.spec.owners[1],
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

    assert!(
        project::assert_project_is_in_phase(&client, &name, ProjectPhase::WaitingForChanges)
            .await
            .is_ok(),
        "project should be in waiting state"
    );

    Ok(())
}
