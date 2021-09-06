use std::cmp::PartialEq;
use std::collections::BTreeMap;

use anyhow::bail;
use anyhow::Context;
use handlebars::Handlebars;
use k8s_openapi::api::core::v1::Secret;

use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use krator::ObjectStatus;
use krator_derive::AdmissionWebhook;

use kube::Client;
use kube::CustomResource;
pub use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;

use crate::self_service;
use crate::self_service::project::states::ProjectPhase;

pub const SECRET_ANNOTATION_KEY: &str = "project.selfservice.innoq.io/operator-access";
pub const SECRET_ANNOTATION_VALUE: &str = "grant";
pub const DEFAULT_MANIFESTS_SECRET: &str = "default-project-manifests";

pub const COPY_ANNOTATION_BASE: &str = "project.selfservice.innoq.io";
pub const COPY_ANNOTATION_COPY_VALUE: &str = "copy";
pub const COPY_ANNOTATION_SKIP_VALUE: &str = "skip";

// TODO: follow up on https://github.com/clux/kube-rs/issues/264#issuecomment-748327959
#[derive(
    AdmissionWebhook,
    CustomResource,
    Serialize,
    Deserialize,
    PartialEq,
    Default,
    Debug,
    Clone,
    JsonSchema,
)]
#[admission_webhook_features(secret, service, admission_webhook_config)]
/// a self service project that will create a namespace per project with the owner having cluster-admin
/// rights in this namespace
#[serde(rename_all = "camelCase")]
#[kube(
    group = "selfservice.innoq.io",
    version = "v1",
    kind = "Project",
    status = "ProjectStatus",
    shortname = "ssp",
    printcolumn = r#"
     {"name":"Owner", "type":"string", "description":"owner of this project", "jsonPath":".spec.owner"},
     {"name":"Private", "type":"string", "description":"whether the project's namespace is private", "jsonPath":".spec.private"},
     {"name":"Age", "type":"date", "description":"how old this resource is", "jsonPath":".metadata.creationTimestamp"},
     {"name":"Phase", "type":"string", "description":"current phase of this resource", "jsonPath":".status.phase"},
     {"name":"Status summary", "type":"string", "description":"current status", "jsonPath":".status.summary"}
  "#
)]
pub struct ProjectSpec {
    /// Owner of this project -- this user will have cluster-admin rights within the created namespace
    /// it must be the user name of this user
    pub owners: Vec<String>,

    /// a map of values that should be templated into manifests that get created
    pub manifest_values: Option<String>,
}

impl Sample for ProjectSpec {
    fn sample() -> Self {
        let manifest_values = r#"
project_repo: github.com/innoq/noqnoqnoq
project_name: self-service-project
"#;

        ProjectSpec {
            owners: vec![
                "superdev@example.com".to_string(),
                "supradev@example.com".to_string(),
            ],
            manifest_values: Some(manifest_values.into()),
        }
    }
}

impl Project {
    // for each project a namespace is created and kubernetes resources are created within this
    // namespace. Project manifest can influence which resources get created (or skipped) via annotations:
    //
    // project.selfservice.innoq.io/<secret-name>.<data-item-name>: copy
    // project.selfservice.innoq.io/<secret-name>: copy (applies to all data items of that secret)
    //
    // project.selfservice.innoq.io/<secret-name>.<data-item-name>: skip
    // project.selfservice.innoq.io/<secret-name>: skip (applies to all data items of that secret)
    //
    // there is an implicit
    //
    // project.selfservice.innoq.io/default-project-manifests: copy
    //
    // for all project
    pub async fn associated_manifests(
        &self,
        client: &Client,
        default_manifests_secret: &str,
        namespace: &str,
    ) -> anyhow::Result<Vec<String>> {
        #[derive(Clone)]
        struct ManifestReference {
            secret_name: String,
            data_item: Option<String>,
        }

        let mut manifest_references = vec![ManifestReference {
            secret_name: default_manifests_secret.to_string(),
            data_item: None,
        }];

        let mut skip_manifest_references = None;

        if let Some(annotations) = &self.metadata.annotations {
            let mut copy_manifests = annotations
                .iter()
                .filter(|(key, value)| {
                    key.starts_with(COPY_ANNOTATION_BASE) && *value == COPY_ANNOTATION_COPY_VALUE
                })
                .map(|(ref key, _)| {
                    let secret_and_item = key
                        .to_string()
                        .replace(&format!("{}/", COPY_ANNOTATION_BASE), "");

                    // we do a splitn here as the data item name can well contain a '.' ... therefore secret
                    // names must not contain a '.' in their name (even thought it's allowed in kubernetes)
                    let secret_and_item = secret_and_item.splitn(2, '.').collect::<Vec<_>>();

                    ManifestReference {
                        secret_name: secret_and_item[0].to_string(),
                        data_item: secret_and_item.get(1).map(|x| x.to_string()),
                    }
                })
                .collect::<Vec<_>>();

            manifest_references.append(&mut copy_manifests);

            let skip_manifests = annotations
                .iter()
                .filter(|(key, value)| {
                    key.starts_with(COPY_ANNOTATION_BASE) && *value == COPY_ANNOTATION_SKIP_VALUE
                })
                .map(|(ref key, _)| {
                    let secret_and_item = key
                        .to_string()
                        .replace(&format!("{}/", COPY_ANNOTATION_BASE), "");
                    let secret_and_item = secret_and_item.splitn(2, '.').collect::<Vec<_>>();

                    ManifestReference {
                        secret_name: secret_and_item[0].to_string(),
                        data_item: secret_and_item.get(1).map(|x| x.to_string()),
                    }
                })
                .collect::<Vec<_>>();

            if !skip_manifests.is_empty() {
                skip_manifest_references = Some(skip_manifests);
            }
        }

        let api: kube::Api<Secret> = kube::Api::namespaced(client.to_owned(), namespace);

        let skip = |reference: &ManifestReference| -> bool {
            if let Some(ref skip_manifest_references) = skip_manifest_references {
                if skip_manifest_references
                    .iter()
                    .any(|skip_manifest_reference| {
                        reference.secret_name == skip_manifest_reference.secret_name
                            && (reference.data_item == skip_manifest_reference.data_item
                                || skip_manifest_reference.data_item == None)
                    })
                {
                    return true;
                }
            }
            false
        };

        let mut manifest_yaml_sources = vec![];
        for reference in manifest_references.iter() {
            if skip(reference) {
                continue;
            }

            let secret = api.get(&reference.secret_name).await.context(format!(
                "annotation '{}/{}.{}: copy' not possible: secret with name '{}' does not exist",
                COPY_ANNOTATION_BASE,
                reference.secret_name,
                reference.data_item.as_ref().unwrap_or(&"".to_string()),
                reference.secret_name
            ))?;

            if let Some(data_item) = &reference.data_item {
                let missing_item_message = format!(
                        "annotation '{}/{}.{}: copy' not possible: secret '{}' does not contain a data item named '{}'",
                        COPY_ANNOTATION_BASE,
                        reference.secret_name,
                        data_item,
                        reference.secret_name,
                        data_item
                    );

                let manifest = secret.data.context(missing_item_message.clone())?;
                let manifest = manifest
                    .get(data_item)
                    .context(missing_item_message.clone())?
                    .to_owned();

                let manifest =
                    String::from_utf8(manifest.to_owned().0).unwrap_or_else(|_| String::from(""));
                let rendered_manifest = self.render(&manifest, data_item).context(format!(
                    "error rendering '{}' from secret '{}':",
                    data_item, reference.secret_name
                ))?;
                manifest_yaml_sources.push(rendered_manifest);
            } else {
                // copy all data items (if any) of this secret
                if let Some(manifests) = secret.data {
                    for (data_item, manifest) in manifests.iter() {
                        if skip(&ManifestReference {
                            secret_name: reference.secret_name.clone(),
                            data_item: Some(data_item.to_owned()),
                        }) {
                            continue;
                        }

                        let manifest = String::from_utf8(manifest.to_owned().0)
                            .unwrap_or_else(|_| String::from(""));
                        let rendered_manifest = self.render(
                            &manifest,
                            &format!("{}/{}", reference.secret_name, data_item),
                        )?;
                        manifest_yaml_sources.push(rendered_manifest);
                    }
                }
            }
        }
        Ok(manifest_yaml_sources)
    }

    pub fn render(&self, template: &str, name: &str) -> anyhow::Result<String> {
        let mut template_data = match &self.spec.manifest_values {
            Some(values) => {
                match serde_yaml::from_str(values) {
                    Ok(yaml) => {
                        // check if this is _just_ a string -- this is accepted by the parser, but we can be kind of certain
                        // that this is a wrong usage of manifestValues
                        if let serde_yaml::Value::Mapping(mapping) = &yaml {
                            mapping.to_owned()
                        } else {
                            let value_type = match &yaml {
                                serde_yaml::Value::Number(_) => "a number",
                                serde_yaml::Value::Null => "a null-value",
                                serde_yaml::Value::Bool(_) => "a boolean",
                                serde_yaml::Value::String(_) => "a string",
                                serde_yaml::Value::Sequence(_) => "an array",
                                _ => std::unreachable!()
                            };
                            bail!("Invalid project spec: property manifestValues must be a string that represents a yaml mapping, got {} with value '{}'",value_type, values)
                        }
                    },
                    Err(e) => bail!("Invalid project spec: error parsing manifestValues which must be a string that represents a yaml mapping, got '{}':\n{}", values, e),
                }
            }
            _ => Mapping::new(),
        };

        template_data.insert(
            serde_yaml::to_value("__PROJECT_NAME__").unwrap(),
            serde_yaml::to_value(self.metadata.name.as_ref().unwrap()).unwrap(),
        );
        template_data.insert(
            serde_yaml::to_value("__PROJECT_OWNERS__").unwrap(),
            serde_yaml::to_value(&self.spec.owners).unwrap(),
        );

        let mut reg = Handlebars::new();
        reg.set_strict_mode(true);
        reg.register_template_string(name, &template)?;

        match reg.render(name, &template_data) {
            Ok(manifest) => Ok(manifest),
            Err(e) => bail!(
                "{} (did you provide all necessary manifestValues in the project spec?)",
                e
            ),
        }
    }
}

impl From<&Project> for OwnerReference {
    fn from(p: &Project) -> OwnerReference {
        OwnerReference {
            api_version: p.api_version.clone(),
            block_owner_deletion: None,
            controller: Some(true),
            kind: p.kind.clone(),
            name: p.metadata.name.clone().unwrap(),
            uid: p.metadata.uid.clone().unwrap(),
        }
    }
}

impl Default for Project {
    fn default() -> Self {
        Project::new("", ProjectSpec::default())
    }
}

impl Sample for Project {
    fn sample() -> Self {
        let mut project = Project::new("sample-self-service-project", ProjectSpec::sample());
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "project.selfservice.innoq.io/argocd.project".to_string(),
            "copy".to_string(),
        );
        annotations.insert(
            "project.selfservice.innoq.io/gitlab-container-registry-secrets.private-key"
                .to_string(),
            "skip".to_string(),
        );

        project.metadata.annotations = Some(annotations);

        project
    }
}

impl PartialEq for Project {
    fn eq(&self, other: &Project) -> bool {
        self.metadata.name == other.metadata.name && self.spec == other.spec
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
#[doc = "Reflects the status of the current self service project"]
pub struct ProjectStatus {
    pub(crate) phase: Option<ProjectPhase>,
    pub message: Option<String>,
    pub summary: Option<String>,
}

impl ObjectStatus for ProjectStatus {
    fn json_patch(&self) -> serde_json::Value {
        debug!("json_patch called {:?}", self);
        // Generate a map containing only set fields.

        let mut status = serde_json::Map::new();

        if let Some(phase) = self.phase.clone() {
            status.insert("phase".to_string(), serde_json::json!(phase));
        };

        if let Some(message) = self.message.clone() {
            status.insert("message".to_string(), serde_json::Value::String(message));
        };

        if let Some(summary) = self.summary.clone() {
            status.insert("summary".to_string(), serde_json::Value::String(summary));
        };

        // Create status patch with map.
        serde_json::json!({ "status": serde_json::Value::Object(status) })
    }

    fn failed(e: &str) -> ProjectStatus {
        let message = format!("error: {}", e);
        ProjectStatus {
            summary: Some(self_service::project::shorten_string(&message)),
            message: Some(message),
            phase: None,
        }
    }
}

pub trait Sample {
    fn sample() -> Self;
}
