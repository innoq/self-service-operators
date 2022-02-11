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

#[macro_use]
extern crate log;

use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::api;

pub mod postgres;
pub mod project;

pub async fn install_crd(
    client: &kube::Client,
    crd: &CustomResourceDefinition,
) -> anyhow::Result<CustomResourceDefinition> {
    let crds: kube::Api<CustomResourceDefinition> = kube::Api::all(client.clone());
    let pp = api::PostParams::default();

    match crds.create(&pp, crd).await {
        Ok(crd) => {
            info!(
                "Created {} ({:?})",
                crd.metadata.name.as_ref().unwrap(),
                crd.status.as_ref().unwrap()
            );
            debug!("Created CRD: {:?}", crd.spec);
            Ok(crd)
        }
        Err(e) => {
            error!(
                "error installing crd:\n{}",
                serde_yaml::to_string(&crd).unwrap()
            );
            Err(e.into())
        } // any other case is probably bad
    }
}
