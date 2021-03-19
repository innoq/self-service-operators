use anyhow::ensure;
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

use anyhow::Context;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use serde::Deserialize;

#[derive(Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResourceInfo {
    metadata: ObjectMeta,
    api_version: String,
    kind: String,
}

pub async fn resource_path(
    client: &kube::Client,
    yaml_manifest: &str,
    namespace: &str,
) -> anyhow::Result<String> {
    let resource_info: ResourceInfo = serde_yaml::from_str(yaml_manifest)?;

    let is_core_api_resource = !resource_info.api_version.contains("/");

    let available_resources = if is_core_api_resource {
        client
            .list_core_api_resources(resource_info.api_version.as_str())
            .await?
    } else {
        client
            .list_api_group_resources(resource_info.api_version.as_str())
            .await?
    };

    let resource = available_resources
        .resources
        .into_iter()
        .find(|resource| resource.kind == resource_info.kind)
        .with_context(|| {
            format!(
                "api version {} not available in kubernetes cluster",
                resource_info.api_version
            )
        })?;

    ensure!(
        resource.namespaced,
        "only namespaced resources are supported: {}/{} with name '{}' is a cluster resource",
        resource_info.api_version,
        resource_info.kind,
        resource_info.metadata.name.unwrap()
    );

    ensure!(
        resource_info.metadata.namespace.is_none(),
        "setting namespace forbidden: resource {}/{} with name '{}' has namespace set explicitly to '{}'",
        resource_info.api_version,
        resource_info.kind,
        resource_info.metadata.name.unwrap(),
        resource_info.metadata.namespace.unwrap()
    );

    if is_core_api_resource {
        Ok(format!(
            "/api/{version}/namespaces/{namespace}/{resource}",
            version = &resource_info.api_version,
            namespace = namespace,
            resource = &resource.name
        ))
    } else {
        Ok(format!(
            "/apis/{api_version}/namespaces/{namespace}/{resource}",
            api_version = &resource_info.api_version,
            namespace = namespace,
            resource = &resource.name
        ))
    }
}
