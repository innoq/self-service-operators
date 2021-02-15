use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube;
use kube::api;

pub async fn install_crd(
    client: &kube::Client,
    crd: &CustomResourceDefinition,
) -> anyhow::Result<()> {
    let crds: kube::Api<CustomResourceDefinition> = kube::Api::all(client.clone());
    let pp = api::PostParams::default();

    match crds.create(&pp, &crd).await {
        Ok(o) => {
            info!("Created {} ({:?})", api::Meta::name(&o), o.status.unwrap());
            debug!("Created CRD: {:?}", o.spec);
            Ok(())
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
