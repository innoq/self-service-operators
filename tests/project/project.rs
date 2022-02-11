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

use core::option::Option::None;
use core::result::Result::{Err, Ok};

use kube::api::PostParams;
use kube::{Resource, ResourceExt};
use serial_test::serial;

use self_service_operators::project::{Project, ProjectSpec};

use crate::project;
use self_service_operators::project::states::ProjectPhase;

#[tokio::test]
#[serial]
async fn it_is_possible_to_update_project() -> anyhow::Result<()> {
    let (client, _) = project::before_each().await?;

    let name = crate::random_name("update-project");
    let _ = project::install_project(&client, &name).await?;

    let api: kube::Api<Project> = kube::Api::all(client.clone());

    let _ =
        project::assert_project_is_in_phase(&client, &name, ProjectPhase::WaitingForChanges).await;

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

    Ok(())
}
