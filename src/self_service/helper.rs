use std::path::Path;

use crate::project;
use crate::project::Project;
use anyhow::bail;
use anyhow::ensure;
use anyhow::Context;
use http::Request;
use k8s_openapi::api::core::v1::Secret;
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

pub async fn get_manifests_secret(
    client: &kube::Client,
    secret_name: &str,
    namespace: &str,
) -> anyhow::Result<Secret> {
    let secret_api: kube::Api<Secret> = kube::Api::namespaced(client.to_owned(), &namespace);

    let secret = secret_api.get(secret_name).await?;

    let annotation = secret
        .metadata
        .annotations
        .as_ref()
        .and_then(|annotations| annotations.get(crate::project::SECRET_ANNOTATION_KEY));

    ensure!(
        annotation.is_some() && annotation.unwrap() == crate::project::SECRET_ANNOTATION_VALUE,
        "Error accessing secret '{}': only secrets with the annotation '{}: {}' can be accessed by the project operator",
        secret_name,
        crate::project::SECRET_ANNOTATION_KEY,
        crate::project::SECRET_ANNOTATION_VALUE
    );

    Ok(secret)
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

pub fn add_resource_version_to_yaml_manifest(
    yaml_manifest: &str,
    existing_manifest: &str,
) -> anyhow::Result<String> {
    // let resource_version = serde_yaml::to_value(&resource_version);

    let mut yaml: serde_yaml::Value = serde_yaml::from_str(yaml_manifest)?;
    let existing_yaml: serde_yaml::Value = serde_yaml::from_str(existing_manifest)?;

    yaml["metadata"]["resourceVersion"] = serde_yaml::Value::String(
        existing_yaml["metadata"]["resourceVersion"]
            .as_str()
            .unwrap()
            .to_string(),
    );

    let versioned_manifest_as_string = serde_yaml::to_string(&yaml)?;

    Ok(versioned_manifest_as_string)
}

pub fn shorten_string(s: &str) -> String {
    let max_length = 50;
    let mut s = s.to_string();
    if s.len() > max_length {
        s = s.replace("\n", " ");
        s.truncate(max_length - 3);
        s.push_str("...");
    }

    s
}

pub async fn apply_yaml_manifest(
    client: &kube::Client,
    yaml_manifest: &str,
    project: &Project,
    project_manifest: bool,
) -> anyhow::Result<()> {
    let resource_path = resource_path(
        &client,
        yaml_manifest,
        &project.metadata.name.as_ref().unwrap(),
        project_manifest,
    )
    .await?;

    let manifest = add_owner_to_yaml_manifest(yaml_manifest, &project)?;

    let get_request = Request::builder()
        .uri(&resource_path)
        .method("GET")
        .body("".into())
        .unwrap();

    let request;
    if client.request_text(get_request).await.is_ok() {
        request = Request::builder()
            .uri(format!(
                "{}?fieldManager=self-service-operator&force=true",
                &resource_path
            ))
            .method("PATCH")
            .header("Content-Type", "application/apply-patch+yaml")
            .body(manifest.into())
            .unwrap();
    } else {
        let resource_path = Path::new(&resource_path).parent().unwrap();
        request = Request::builder()
            .uri(format!(
                "{}?fieldManager=self-service-operator&force=true",
                &resource_path.display().to_string()
            ))
            .method("POST")
            .header("Content-Type", "application/yaml")
            .body(manifest.clone().into())
            .unwrap();
    }

    match client.request_text(request).await {
        Ok(_) => Ok(()),
        Err(e) => bail!("error applying manifest: {}", e),
    }
}

pub async fn resource_path(
    client: &kube::Client,
    yaml_manifest: &str,
    namespace: &str,
    project_manifest: bool,
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

    if project_manifest {
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
    }

    let namespace_specifier = if resource.namespaced {
        format!("namespaces/{}/", namespace)
    } else {
        "".to_string()
    };

    if is_core_api_resource {
        Ok(format!(
            "/api/{version}/{namespace_specifier}{resource}/{name}",
            version = &resource_info.api_version,
            namespace_specifier = namespace_specifier,
            resource = &resource.name,
            name = resource_info.metadata.name.unwrap()
        ))
    } else {
        Ok(format!(
            "/apis/{api_version}/{namespace_specifier}{resource}/{name}",
            api_version = &resource_info.api_version,
            namespace_specifier = namespace_specifier,
            resource = &resource.name,
            name = resource_info.metadata.name.unwrap()
        ))
    }
}

pub fn is_owned_by_project<R>(project: &project::Project, resource: &R) -> bool
where
    R: kube::Resource + k8s_openapi::Resource,
{
    if resource.meta().owner_references.is_none() {
        return false;
    }

    if let Some(owners) = resource.meta().owner_references.as_ref() {
        if owners.is_empty() {
            return false;
        }

        let owner = &owners[0];

        return owner.api_version == project.api_version
            && owner.controller == Some(true)
            && owner.kind == project.kind
            && owner.name == *project.metadata.name.as_ref().unwrap()
            && owner.uid == project.metadata.uid.clone().unwrap_or_else(String::new);
    }

    true
}
