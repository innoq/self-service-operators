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

use std::time::Duration;

use anyhow::bail;
use k8s_openapi::api::core::v1::Namespace;
use kube::api::DeleteParams;
use serial_test::serial;
use tokio::select;
use tokio::time;

use self_service_operators::project::Project;

use crate::project;
use crate::WaitForState;

#[tokio::test]
#[serial]
async fn it_creates_and_deletes_namespace() -> anyhow::Result<()> {
    let timeout_secs = 60;
    let (client, _) = project::before_each().await?;

    let name = crate::random_name("namespace-test");
    let project = project::install_project(&client, &name).await?;

    let ns_api: kube::Api<Namespace> = kube::Api::all(client.clone());
    let project_namespace = ns_api.get(&name).await?;

    assert!(
        project::assert_is_owned_by_project(&project, &project_namespace).is_ok(),
        "namespace should be owned by project"
    );

    let wait_for_project_deleted_handle = crate::wait_for_state(
        &kube::Api::<Project>::all(client.clone()),
        &name,
        WaitForState::Deleted,
    );

    let wait_for_namespace_deleted_handle =
        crate::wait_for_state(&ns_api, &name, WaitForState::Deleted);

    assert!(
        kube::Api::<Project>::all(client.clone())
            .delete(&name, &DeleteParams::default())
            .await
            .is_ok(),
        "deleting project should work"
    );

    select! {
    res = futures::future::try_join(wait_for_project_deleted_handle,wait_for_namespace_deleted_handle) => {
        match res {
            Ok(_) => (),
            Err(e) => bail!("error deleting namespace {}: {}", name, e)
        }
    },
        _ = time::sleep(Duration::from_secs(timeout_secs)) => bail!("deleting project {} deletes project and namespace within {} seconds", name, timeout_secs)
    }

    Ok(())
}
