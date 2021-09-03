#[macro_use]
extern crate log;

use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::api;

pub mod self_service;

pub async fn install_crd(
    client: &kube::Client,
    crd: &CustomResourceDefinition,
) -> anyhow::Result<CustomResourceDefinition> {
    let crds: kube::Api<CustomResourceDefinition> = kube::Api::all(client.clone());
    let pp = api::PostParams::default();

    match crds.create(&pp, &crd).await {
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
