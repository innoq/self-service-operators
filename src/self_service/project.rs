use anyhow::{anyhow, bail};

use futures::{StreamExt, TryStreamExt};
use handlebars::Handlebars;
use k8s_openapi::ByteString;
use kube::Client;
use std::cmp::PartialEq;
use std::{sync::Arc, time::Duration};

use anyhow::Context;
use krator::{
    admission::AdmissionTls, Manifest, ObjectState, ObjectStatus, Operator, State, Transition,
    TransitionTo,
};
use kube::{Api, CustomResource, Resource};
// use kube::api::ListParams;
use super::Sample;
use crate::helper;
use crate::project::ProjectPhase::Initializing;
use k8s_openapi::api::core::v1::{Namespace, Secret};
use k8s_openapi::api::rbac::v1::{
    ClusterRole, ClusterRoleBinding, PolicyRule, RoleBinding, RoleRef, Subject,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Status;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ListMeta, OwnerReference};
use krator::admission::AdmissionResult;
use krator_derive::AdmissionWebhook;
use kube::api::{ListParams, ObjectMeta, PostParams, WatchEvent};
pub use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tokio::sync::RwLock;

pub const OWNER_ROLE_BINDING_NAME: &str = "self-service-project-owner";
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
     {"name":"Private", "type":"string", "description":"whether the projects's namespace is private", "jsonPath":".spec.private"},
     {"name":"Age", "type":"date", "description":"how old this resource is", "jsonPath":".metadata.creationTimestamp"},
     {"name":"Phase", "type":"string", "description":"current phase of this resource", "jsonPath":".status.phase"},
     {"name":"Status summary", "type":"string", "description":"current status", "jsonPath":".status.summary"}
  "#
)]
pub struct ProjectSpec {
    /// Owner of this project -- this user will have cluster-admin rights within the created namespace
    /// it must be the user name of this user
    pub owner: String,

    /// a map of values that should be templated into manifests that get created
    pub manifest_values: Option<String>,
}

// TODO: this is just for nicer test output ...
impl k8s_openapi::Resource for Project {
    const GROUP: &'static str = "selfservice.innoq.io";
    const API_VERSION: &'static str = "selfservice.innoq.io/v1";
    const KIND: &'static str = "Project";
    const VERSION: &'static str = "v1";
}

impl Sample for ProjectSpec {
    fn sample() -> Self {
        let manifest_values = r#"
project_repo: github.com/innoq/noqnoqnoq
project_name: self-service-project
"#;

        ProjectSpec {
            owner: "superdev@example.com".to_string(),
            manifest_values: Some(manifest_values.into()),
        }
    }
}

impl Project {
    pub fn owner_cluster_role_name(&self) -> String {
        format!(
            "selfservice:project:owner:{}",
            self.metadata.name.as_ref().unwrap()
        )
    }

    pub fn rbac_manifests(
        &self,
        default_owner_cluster_role: &str,
    ) -> (RoleBinding, ClusterRole, ClusterRoleBinding) {
        let owner_role_binding_name = OWNER_ROLE_BINDING_NAME.to_string();

        let rolebinding = RoleBinding {
            metadata: ObjectMeta {
                name: Some(owner_role_binding_name),
                owner_references: Some(vec![OwnerReference::from(self)]),
                ..Default::default()
            },
            role_ref: RoleRef {
                api_group: "rbac.authorization.k8s.io".to_string(),
                kind: "ClusterRole".to_string(),
                name: default_owner_cluster_role.to_string(),
            },
            subjects: Some(vec![Subject {
                api_group: None,
                kind: "User".to_string(),
                name: self.spec.owner.clone(),
                namespace: None,
            }]),
        };

        let owner_cluster_role = ClusterRole {
            aggregation_rule: None,
            metadata: ObjectMeta {
                name: Some(self.owner_cluster_role_name()),
                owner_references: Some(vec![OwnerReference::from(self)]),
                ..Default::default()
            },
            rules: Some(vec![PolicyRule {
                api_groups: Some(vec![Project::group(&()).to_string()]),
                non_resource_urls: None,
                resource_names: Some(vec![self.meta().name.as_ref().unwrap().to_string()]),
                resources: Some(vec![Project::plural(&()).to_string()]),
                verbs: vec![
                    "get".to_string(),
                    "list".to_string(),
                    "watch".to_string(),
                    "create".to_string(),
                    "update".to_string(),
                    "patch".to_string(),
                    "delete".to_string(),
                ],
            }]),
        };

        let owner_cluster_role_binding = ClusterRoleBinding {
            metadata: ObjectMeta {
                name: Some(self.owner_cluster_role_name()),
                owner_references: Some(vec![OwnerReference::from(self)]),
                ..Default::default()
            },
            role_ref: RoleRef {
                api_group: ClusterRole::group(&()).to_string(),
                kind: ClusterRole::kind(&()).to_string(),
                name: self.owner_cluster_role_name(),
            },
            subjects: Some(vec![Subject {
                api_group: None,
                kind: "User".to_string(),
                name: self.spec.owner.clone(),
                namespace: None,
            }]),
        };

        (rolebinding, owner_cluster_role, owner_cluster_role_binding)
    }

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
    // for all projects
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

                        let rendered_manifest = self.render(
                            manifest,
                            &format!("{}/{}", reference.secret_name, data_item),
                        )?;
                        manifest_yaml_sources.push(rendered_manifest);
                    }
                }
            }
        }
        Ok(manifest_yaml_sources)
    }

    fn render(&self, template: &ByteString, name: &str) -> anyhow::Result<String> {
        let template =
            String::from_utf8(template.to_owned().0).unwrap_or_else(|_| String::from(""));

        let template_data: serde_yaml::Mapping = match &self.spec.manifest_values {
            Some(manifest_values) => match serde_yaml::from_str(&manifest_values) {
                Ok(value) => {
                    // check if this is _just_ a string -- this is accepted by the parser, but we can be kind of certain
                    // that this is a wrong usage of manifestValues
                    if let serde_yaml::Value::Mapping(mapping) = &value {
                        mapping.to_owned()
                    } else {
                        let value_type = match &value {
                            serde_yaml::Value::Number(_) => "a number",
                            serde_yaml::Value::Null => "a null-value",
                            serde_yaml::Value::Bool(_) => "a boolean",
                            serde_yaml::Value::String(_) => "a string",
                            serde_yaml::Value::Sequence(_) => "an array",
                            _ => std::unreachable!()
                        };
                        bail!("Invalid project spec: property manifestValues must be a string that represents a yaml mapping, got {} with value '{}'",value_type, manifest_values)
                    }
                },
                Err(e) => bail!("Invalid project spec: error parsing manifestValues which must be a string that represents a yaml mapping, got '{}':\n{}", manifest_values, e),
            },
            None => serde_yaml::Mapping::new(),
        };

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
enum ProjectPhase {
    Initializing,
    CreatingNamespace,
    SettingUpRBACPermissions,
    ApplyingManifests,
    FailedDueToError,
    WaitingForChanges,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
#[doc = "Reflects the status of the current self service project"]
pub struct ProjectStatus {
    phase: Option<ProjectPhase>,
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
            summary: Some(helper::shorten_string(&message)),
            message: Some(message),
            phase: None,
        }
    }
}

pub struct ProjectState {
    name: String,
    _spec: ProjectSpec,
    error: String,
}

#[async_trait::async_trait]
impl ObjectState for ProjectState {
    type Manifest = Project;
    type Status = ProjectStatus;
    type SharedState = SharedState;
    async fn async_drop(self, _shared: &mut Self::SharedState) {}
}

#[derive(Debug, Default)]
/// New project was detected
pub struct NewProject;

impl TransitionTo<CreateNamespace> for NewProject {}
#[async_trait::async_trait]
impl State<ProjectState> for NewProject {
    async fn next(
        self: Box<Self>,
        _shared: Arc<RwLock<SharedState>>,
        state: &mut ProjectState,
        _manifest: Manifest<Project>,
    ) -> Transition<ProjectState> {
        debug!("next() in NewProject");
        info!("new project named {} detected", state.name);
        Transition::next(self, CreateNamespace)
    }

    async fn status(
        &self,
        _state: &mut ProjectState,
        manifest: &Project,
    ) -> anyhow::Result<ProjectStatus> {
        debug!("status() in NewProject");
        Ok(ProjectStatus {
            phase: Some(Initializing),
            message: Some(format!(
                "new project {} detected",
                manifest.metadata.name.as_ref().unwrap()
            )),
            summary: Some(format!(
                "new project {} detected",
                manifest.metadata.name.as_ref().unwrap()
            )),
        })
    }
}

#[derive(Debug, Default)]
/// Project is creating a namespace
struct CreateNamespace;
impl TransitionTo<SetupRBACPermissions> for CreateNamespace {}
impl TransitionTo<Error> for CreateNamespace {}
#[async_trait::async_trait]
impl State<ProjectState> for CreateNamespace {
    async fn next(
        self: Box<Self>,
        shared: Arc<RwLock<SharedState>>,
        state: &mut ProjectState,
        manifest: Manifest<Project>,
    ) -> Transition<ProjectState> {
        info!("creating namespace {}", &state.name);

        let api: kube::Api<Namespace> = kube::Api::all(shared.read().await.client.clone());
        let project = manifest.latest();
        let name = project.clone().metadata.name.unwrap();

        if let Ok(namespace) = api.get(&name).await {
            if helper::is_owned_by_project(&project, &namespace) {
                return Transition::next(self, SetupRBACPermissions);
            } else {
                state.error = format!(
                    "namespace '{}' exists but does not belong to project '{}'",
                    &name, &name
                );
                return Transition::next(self, Error);
            }
        }

        let namespace = Namespace {
            metadata: ObjectMeta {
                name: Some(name),
                owner_references: Some(vec![OwnerReference::from(&project)]),
                ..Default::default()
            },
            ..Default::default()
        };

        if let Err(e) = api.create(&PostParams::default(), &namespace).await {
            state.error = format!("error creating namespace {}: {}", state.name, e.to_string());
            Transition::next(self, Error)
        } else {
            Transition::next(self, SetupRBACPermissions)
        }
    }

    async fn status(
        &self,
        state: &mut ProjectState,
        _manifest: &Project,
    ) -> anyhow::Result<ProjectStatus> {
        debug!("status() in CreateNamespace");
        Ok(ProjectStatus {
            phase: Some(ProjectPhase::CreatingNamespace),
            message: Some(format!("creating namespace {}", state.name)),
            summary: Some(format!("creating namespace {}", state.name)),
        })
    }
}

#[derive(Debug, Default)]
/// Something went wrong
struct Error;
impl TransitionTo<Error> for Error {}

#[async_trait::async_trait]
impl State<ProjectState> for Error {
    async fn next(
        self: Box<Self>,
        _shared: Arc<RwLock<SharedState>>,
        state: &mut ProjectState,
        _manifest: Manifest<Project>,
    ) -> Transition<ProjectState> {
        info!("error {}", &state.name);

        tokio::time::sleep(std::time::Duration::from_secs(60)).await;

        Transition::next(self, Error)
    }

    async fn status(
        &self,
        state: &mut ProjectState,
        _manifest: &Project,
    ) -> anyhow::Result<ProjectStatus> {
        debug!("status() in Error");
        let message = format!("error: {}", state.error);
        Ok(ProjectStatus {
            phase: Some(ProjectPhase::FailedDueToError),
            summary: Some(helper::shorten_string(&message)),
            message: Some(message),
        })
    }
}

#[derive(Debug, Default)]
/// Project is sleeping.
struct SetupRBACPermissions;
impl TransitionTo<ApplyManifests> for SetupRBACPermissions {}
impl TransitionTo<Error> for SetupRBACPermissions {}

#[async_trait::async_trait]
impl State<ProjectState> for SetupRBACPermissions {
    async fn next(
        self: Box<Self>,
        shared: Arc<RwLock<SharedState>>,
        state: &mut ProjectState,
        manifest: Manifest<Project>,
    ) -> Transition<ProjectState> {
        let project = manifest.latest();

        let (rolebinding, owner_cluster_role, owner_cluster_role_binding) =
            project.rbac_manifests(&shared.read().await.default_owner_cluster_role);
        let client = shared.read().await.client.clone();

        let _ = kube::Api::<Namespace>::all(client.clone())
            .get(project.meta().name.as_ref().unwrap())
            .await;

        for (name, manifest) in vec![
            (
                "clusterrole",
                serde_yaml::to_string(&owner_cluster_role).unwrap(),
            ),
            (
                "clusterrolebinding",
                serde_yaml::to_string(&owner_cluster_role_binding).unwrap(),
            ),
            ("rolebinding", serde_yaml::to_string(&rolebinding).unwrap()),
        ]
        .iter()
        {
            if let Err(e) = helper::apply_yaml_manifest(&client, &manifest, &project, false).await {
                state.error = format!("error applying {}: {}", name, e);

                return Transition::next(self, Error);
            }
        }

        Transition::next(self, ApplyManifests)
    }

    async fn status(
        &self,
        _state: &mut ProjectState,
        _manifest: &Project,
    ) -> anyhow::Result<ProjectStatus> {
        debug!("status() in SetupRBACPermissions");
        Ok(ProjectStatus {
            phase: Some(ProjectPhase::SettingUpRBACPermissions),
            message: Some("setting up permissions".to_string()),
            summary: Some("setting up permissions".to_string()),
        })
    }
}

#[derive(Debug, Default)]
struct ApplyManifests;
impl TransitionTo<WaitForChanges> for ApplyManifests {}
impl TransitionTo<Error> for ApplyManifests {}

#[async_trait::async_trait]
impl State<ProjectState> for ApplyManifests {
    async fn next(
        self: Box<Self>,
        shared: Arc<RwLock<SharedState>>,
        state: &mut ProjectState,
        manifest: Manifest<Project>,
    ) -> Transition<ProjectState> {
        let shared = shared.read().await;
        let project = manifest.latest();
        let delay = shared.manifest_retry_delay;

        let manifests = project
            .associated_manifests(
                &shared.client,
                &shared.default_manifests_secret,
                &shared.default_ns,
            )
            .await;

        match manifests {
            Err(e) => {
                state.error = e.to_string();
                return Transition::next(self, Error);
            }

            Ok(manifests) => {
                let mut manifests = manifests
                    .iter()
                    .map(|s| (0, s))
                    .collect::<Vec<(u16, &String)>>();

                let mut iteration = 0;
                let max_retries = 5;
                while let Some((i, manifest)) = manifests.pop() {
                    if let Err(e) =
                        helper::apply_yaml_manifest(&shared.client, &manifest, &project, true).await
                    {
                        if i >= max_retries {
                            state.error = format!(
                                "error installing manifest: giving up after {} retries: {}\nmanifest was:\n{}",
                                i,
                                e.to_string(),
                                &manifest
                            );
                            return Transition::next(self, Error);
                        }
                        if i != iteration {
                            iteration = i;
                            tokio::time::sleep(delay).await;
                        }
                        manifests.insert(0, (i + 1, &manifest));
                    }
                }
            }
        }

        Transition::next(self, WaitForChanges)
    }

    async fn status(
        &self,
        _state: &mut ProjectState,
        _manifest: &Project,
    ) -> anyhow::Result<ProjectStatus> {
        debug!("status() in ApplyManifests");
        Ok(ProjectStatus {
            phase: Some(ProjectPhase::ApplyingManifests),
            message: Some("applying configured manifests".to_string()),
            summary: Some("applying configured manifests".to_string()),
        })
    }
}

#[derive(Debug, Default)]
/// Project is sleeping.
struct WaitForChanges;
impl TransitionTo<WaitForChanges> for WaitForChanges {}
impl TransitionTo<CreateNamespace> for WaitForChanges {}
impl TransitionTo<Error> for WaitForChanges {}

#[async_trait::async_trait]
impl State<ProjectState> for WaitForChanges {
    async fn next(
        self: Box<Self>,
        shared: Arc<RwLock<SharedState>>,
        state: &mut ProjectState,
        manifest: Manifest<Project>,
    ) -> Transition<ProjectState> {
        let lp = &ListParams::default().fields(&format!("metadata.name={}", state.name));
        let mut stream = kube::Api::<Project>::all(shared.read().await.client.clone())
            .watch(lp, &(manifest.latest().metadata.resource_version.unwrap()))
            .await
            .unwrap()
            .boxed();

        match stream.try_next().await {
            Ok(Some(status)) => match status.clone() {
                WatchEvent::Modified(_resource) => {
                    info!("project {} modified", state.name);
                    return Transition::next(self, CreateNamespace);
                }
                WatchEvent::Error(e) => {
                    warn!(
                        "ERROR watching Project with name {}: {}",
                        state.name, e.message
                    );
                }
                _ => debug!(
                    "unimplemented state while watching for changes on Project with name {}: {:?}",
                    state.name, status
                ),
            },
            Err(e) => {
                state.error = e.to_string();
                return Transition::next(self, Error);
            }
            _ => {
                print!("#");
            }
        }

        Transition::next(self, WaitForChanges)
    }

    async fn status(
        &self,
        _state: &mut ProjectState,
        _manifest: &Project,
    ) -> anyhow::Result<ProjectStatus> {
        debug!("status() in WaitForChanges");
        Ok(ProjectStatus {
            phase: Some(ProjectPhase::WaitingForChanges),
            message: Some("waiting for changes".to_string()),
            summary: Some("waiting for changes".to_string()),
        })
    }
}

#[derive(Debug, Default)]
/// Project was released from our care.
pub struct Released;

#[async_trait::async_trait]
impl State<ProjectState> for Released {
    async fn next(
        self: Box<Self>,
        _shared: Arc<RwLock<SharedState>>,
        state: &mut ProjectState,
        _manifest: Manifest<Project>,
    ) -> Transition<ProjectState> {
        debug!("next() in Released / name: {}", state.name);
        Transition::Complete(Ok(()))
    }

    async fn status(
        &self,
        state: &mut ProjectState,
        _manifest: &Project,
    ) -> anyhow::Result<ProjectStatus> {
        Ok(ProjectStatus {
            phase: None,
            message: Some(format!("Bye, {}!", state.name)),
            summary: Some(format!("Bye, {}!", state.name)),
        })
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, JsonSchema)]
pub struct EnvRepo {
    pub repo_url: String,
    pub branch: String,
    pub directory: String,
}

pub struct SharedState {
    client: kube::Client,
    default_manifests_secret: String,
    default_owner_cluster_role: String,
    default_ns: String,
    manifest_retry_delay: Duration,
}

impl Default for SharedState {
    fn default() -> Self {
        SharedState {
            default_manifests_secret: DEFAULT_MANIFESTS_SECRET.to_string(),
            manifest_retry_delay: Duration::from_secs(5),
            ..Default::default()
        }
    }
}

impl SharedState {
    pub fn client(&self) -> kube::Client {
        self.client.clone()
    }
}

#[derive(Clone)]
pub struct ProjectOperator {
    shared: Arc<RwLock<SharedState>>,
}

impl ProjectOperator {
    pub async fn new(
        client: kube::Client,
        default_owner_cluster_role: &str,
        default_ns: &str,
        default_manifests_secret: &str,
        manifest_retry_delay: Duration,
    ) -> anyhow::Result<Self> {
        let shared = Arc::new(RwLock::new(SharedState {
            client: client.clone(),
            default_owner_cluster_role: default_owner_cluster_role.to_string(),
            default_ns: default_ns.to_string(),
            default_manifests_secret: default_manifests_secret.to_string(),
            manifest_retry_delay,
        }));

        if let Err(e) =
            crate::helper::get_manifests_secret(&client, default_manifests_secret, default_ns).await
        {
            bail!(
                    "no Secret with name '{}' in namespace '{}' found (this secret should hold default manifests that get applied in each new namespace): {} -- aborting",
                    default_manifests_secret, default_ns, e);
        }

        let _ = kube::Api::<ClusterRole>::all(client.clone())
            .get(&default_owner_cluster_role)
            .await
            .with_context(|| {
                format!(
                    "no ClusterRole with name '{}' found -- aborting",
                    default_owner_cluster_role
                )
            })?;

        Ok(ProjectOperator { shared })
    }
}

#[async_trait::async_trait]
impl Operator for ProjectOperator {
    type Manifest = Project;
    type Status = ProjectStatus;
    type ObjectState = ProjectState;
    type InitialState = NewProject;
    type DeletedState = Released;

    async fn initialize_object_state(
        &self,
        manifest: &Self::Manifest,
    ) -> anyhow::Result<Self::ObjectState> {
        let name = manifest.meta().name.clone().unwrap();
        let spec = manifest.spec.clone();
        Ok(ProjectState {
            name,
            _spec: spec,
            error: "".to_string(),
        })
    }

    async fn shared_state(&self) -> Arc<RwLock<SharedState>> {
        Arc::clone(&self.shared)
    }

    async fn registration_hook(&self, manifest: Manifest<Self::Manifest>) -> anyhow::Result<()> {
        warn!(
            "REGISTRATION HOOK CALLED {}",
            serde_yaml::to_string(&manifest.latest()).unwrap()
        );
        Ok(())
    }

    async fn admission_hook(&self, project: Self::Manifest) -> AdmissionResult<Self::Manifest> {
        let shared = self.shared.read().await;
        let client = shared.client.clone();
        let default_namespace = shared.default_ns.clone();
        let project_name = project.metadata.name.as_ref().expect("");

        debug!("admission hook: {:?}", project);

        let deny = |msg: String| {
            AdmissionResult::Deny(Status {
                code: Some(409),
                details: None,
                message: Some(msg),
                metadata: ListMeta {
                    ..Default::default()
                },

                reason: None,
                status: Some("Failure".to_string()),
            })
        };

        if let Ok(project_namespace) = Api::<Namespace>::all(client.clone())
            .get(&project_name)
            .await
        {
            if let Some(owner_references) = project_namespace.metadata.owner_references {
                let ns_owned_by_this_project =
                    owner_references.into_iter().any(|owner_reference| {
                        owner_reference.kind == Project::kind(&())
                            && owner_reference.name == *project_name
                    });

                if !ns_owned_by_this_project {
                    return deny(format!(
                        "can't create/update project: a namespace with name '{}' already exists but is not owned by this project",
                        project_name
                    ));
                }
            } else {
                return deny(format!(
                    "can't create project: a namespace with name '{}' already exists",
                    project_name
                ));
            }
        }

        if let Err(e) = project
            .associated_manifests(
                &client,
                &shared.default_manifests_secret,
                &default_namespace,
            )
            .await
        {
            return deny(e.to_string());
        }

        AdmissionResult::Allow(project)
    }

    async fn admission_hook_tls(&self) -> anyhow::Result<AdmissionTls> {
        // TOOD: make dynamic
        let client = self.shared.read().await.client.clone();
        let namespace = &self.shared.read().await.default_ns;

        // TODO: extract as method
        let name = Project::admission_webhook_secret_name();

        match Api::<Secret>::namespaced(client, namespace)
            .get(&name)
            .await
        {
            Ok(secret) => Ok(AdmissionTls::from(&secret).unwrap()),
            Err(e) => Err(anyhow!(e)),
        }
    }

    async fn deregistration_hook(
        &self,
        mut _manifest: Manifest<Self::Manifest>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}
