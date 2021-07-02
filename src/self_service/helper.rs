use crate::project::Project;
use anyhow::ensure;
use anyhow::Context;
use http::Request;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};
use kube;
use kube::api;
use serde::Deserialize;

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

#[derive(Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResourceInfo {
    metadata: ObjectMeta,
    api_version: String,
    kind: String,
}

pub fn add_owner_to_yaml_manifest(
    yaml_manifest: &str,
    project: &Project,
) -> anyhow::Result<String> {
    let owner = serde_yaml::to_value(&OwnerReference::from(project));

    let mut yaml: serde_yaml::Value = serde_yaml::from_str(yaml_manifest)?;

    yaml["metadata"]["ownerReferences"] = serde_yaml::Value::Sequence(vec![owner.unwrap()]);

    let owned_manifest_as_string = serde_yaml::to_string(&yaml)?;

    Ok(owned_manifest_as_string)
}

pub async fn apply_yaml_manifest(
    client: &kube::Client,
    yaml_manifest: &str,
    project: &Project,
) -> anyhow::Result<()> {
    let api_path = resource_path(
        &client,
        yaml_manifest,
        &project.metadata.name.as_ref().unwrap(),
    )
    .await?;

    let manifest_with_owner = add_owner_to_yaml_manifest(yaml_manifest, &project)?;

    let request = Request::builder()
        .uri(api_path)
        .method("POST")
        .header("Content-Type", "application/yaml")
        .body(manifest_with_owner.clone().into())
        .unwrap();

    client
        .request_text(request)
        .await
        .with_context(|| format!("error applying manifest\n{}", manifest_with_owner))?;

    Ok(())
}

pub async fn resource_path(
    client: &kube::Client,
    yaml_manifest: &str,
    namespace: &str,
) -> anyhow::Result<String> {
    let resource_info: ResourceInfo = serde_yaml::from_str(yaml_manifest)?;

    let is_core_api_resource = !resource_info.api_version.contains('/');

    let available_resources = if is_core_api_resource {
        client
            .list_core_api_resources(&(resource_info.api_version))
            .await?
    } else {
        client
            .list_api_group_resources(&(resource_info.api_version))
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
