use anyhow::anyhow;
use futures::{StreamExt, TryStreamExt};
use std::cmp::PartialEq;
use std::sync::Arc;

use anyhow::Context;
use krator::{
    admission::AdmissionTls, Manifest, ObjectState, ObjectStatus, Operator, State, Transition,
    TransitionTo,
};
use kube::{Api, CustomResource};
// use kube::api::ListParams;
use super::Sample;
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
use kube::Resource;
pub use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tokio::sync::RwLock;

pub const OWNER_ROLE_BINDING_NAME: &str = "self-service-project-owner";

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
#[kube(
    group = "selfservice.innoq.io",
    version = "v1",
    kind = "Project",
    status = "ProjectStatus",
    shortname = "ssp",
    printcolumn = r#"
     {"name":"Owner", "type":"string", "description":"owner of this project", "jsonPath":".spec.owner"},
     {"name":"Private", "type":"string", "description":"whether the projects's namespace is private", "jsonPath":".spec.private"},
     {"name":"Age", "type":"date", "description":"how old this resource is", "jsonPath":".metadata.creationTimestamp"}
  "#
)]
pub struct ProjectSpec {
    /// Owner of this project -- this user will have cluster-admin rights within the created namespace
    /// it must be the user name of this user
    pub owner: String,

    /// whether this is a private project or whether other developers should be able to get developers access
    pub private: Option<bool>,
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
        ProjectSpec {
            owner: "superdev@example.com".to_string(),
            private: Some(false),
        }
    }
}

impl Project {
    pub fn owner_cluster_role_name(&self) -> String {
        format!("{}-project-owner", self.metadata.name.as_ref().unwrap())
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
    FailedDueToError,
    WaitingForChanges,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
#[doc = "Reflects the status of the current self service project"]
pub struct ProjectStatus {
    phase: Option<ProjectPhase>,
    message: Option<String>,
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

        // Create status patch with map.
        serde_json::json!({ "status": serde_json::Value::Object(status) })
    }

    fn failed(e: &str) -> ProjectStatus {
        ProjectStatus {
            message: Some(format!("Error occurred: {}", e)),
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

        let namespace = Namespace {
            metadata: ObjectMeta {
                name: Some(project.clone().metadata.name.unwrap()),
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
        Ok(ProjectStatus {
            phase: Some(ProjectPhase::FailedDueToError),
            message: Some(format!("error: {}", state.error)),
        })
    }
}

#[derive(Debug, Default)]
/// Project is sleeping.
struct SetupRBACPermissions;

impl TransitionTo<WaitForChanges> for SetupRBACPermissions {}
impl TransitionTo<Error> for SetupRBACPermissions {}

#[async_trait::async_trait]
impl State<ProjectState> for SetupRBACPermissions {
    async fn next(
        self: Box<Self>,
        shared: Arc<RwLock<SharedState>>,
        state: &mut ProjectState,
        manifest: Manifest<Project>,
    ) -> Transition<ProjectState> {
        info!("setting up rbac permissions namespace {}", &state.name);
        debug!("next() in SetupRBACPermissions / name: {}", state.name);

        let project = manifest.latest();

        let rolebinding = RoleBinding {
            metadata: ObjectMeta {
                name: Some(OWNER_ROLE_BINDING_NAME.to_string()),
                owner_references: Some(vec![OwnerReference::from(&project)]),
                ..Default::default()
            },
            role_ref: RoleRef {
                api_group: "rbac.authorization.k8s.io".to_string(),
                kind: "ClusterRole".to_string(),
                name: shared.read().await.default_owner_cluster_role.clone(),
            },
            subjects: Some(vec![Subject {
                api_group: None,
                kind: "User".to_string(),
                name: project.clone().spec.owner,
                namespace: None,
            }]),
            ..Default::default()
        };
        {
            let api: kube::Api<RoleBinding> =
                kube::Api::namespaced(shared.read().await.client.clone(), &state.name);
            if let Err(e) = api.create(&PostParams::default(), &rolebinding).await {
                state.error = format!(
                    "error creating rolebinding {} in namespace {}: {}",
                    OWNER_ROLE_BINDING_NAME,
                    state.name,
                    e.to_string()
                );
                return Transition::next(self, Error);
            }
        }

        let owner_cluster_role = ClusterRole {
            aggregation_rule: None,
            metadata: ObjectMeta {
                name: Some(project.owner_cluster_role_name()),
                owner_references: Some(vec![OwnerReference::from(&project)]),
                ..Default::default()
            },
            rules: Some(vec![PolicyRule {
                api_groups: Some(vec![Project::group(&()).to_string()]),
                non_resource_urls: None,
                resource_names: Some(vec![state.name.clone()]),
                resources: Some(vec![Project::kind(&()).to_string()]),
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
        let api: kube::Api<ClusterRole> = kube::Api::all(shared.read().await.client.clone());
        if let Err(e) = api
            .create(&PostParams::default(), &owner_cluster_role)
            .await
        {
            state.error = format!(
                "error creating owner cluster role {}: {}",
                project.owner_cluster_role_name(),
                e.to_string()
            );
            return Transition::next(self, Error);
        }

        let owner_cluster_role_binding = ClusterRoleBinding {
            metadata: ObjectMeta {
                name: Some(project.owner_cluster_role_name()),
                owner_references: Some(vec![OwnerReference::from(&project)]),
                ..Default::default()
            },
            role_ref: RoleRef {
                api_group: ClusterRole::group(&()).to_string(),
                kind: ClusterRole::kind(&()).to_string(),
                name: project.owner_cluster_role_name(),
            },
            subjects: Some(vec![Subject {
                api_group: None,
                kind: "User".to_string(),
                name: project.spec.owner.clone(),
                namespace: None,
            }]),
        };
        {
            let api: kube::Api<ClusterRoleBinding> =
                kube::Api::all(shared.read().await.client.clone());
            if let Err(e) = api
                .create(&PostParams::default(), &owner_cluster_role_binding)
                .await
            {
                state.error = format!(
                    "error creating owner cluster role binding {}: {}",
                    project.owner_cluster_role_name(),
                    e.to_string()
                );
                return Transition::next(self, Error);
            }
        }

        Transition::next(self, WaitForChanges)
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
        let ssp_api = kube::Api::<Project>::all(shared.read().await.client.clone());
        let lp = &ListParams::default().fields(&format!("metadata.name={}", state.name));
        let mut stream = ssp_api
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
    default_owner_cluster_role: String,
    default_ns: String,
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
    ) -> anyhow::Result<Self> {
        let shared = Arc::new(RwLock::new(SharedState {
            client: client.clone(),
            default_owner_cluster_role: default_owner_cluster_role.to_string(),
            default_ns: default_ns.to_string(),
        }));

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
        AdmissionResult::Deny(Status {
            code: Some(409),
            details: None,
            message: Some(format!(
                "can't create project: a namespace with name '{}' already exists",
                project.metadata.name.expect("")
            )),
            metadata: ListMeta {
                ..Default::default()
            },
            reason: None,
            status: Some("Failure".to_string()),
        })
    }

    async fn admission_hook_tls(&self) -> anyhow::Result<AdmissionTls> {
        // TOOD: make dynamic
        let client = self.shared.read().await.client.clone();
        let namespace = &self.shared.read().await.default_ns;

        let secret_api = Api::<Secret>::namespaced(client, namespace);
        // TODO: extract as method
        let name = Project::admission_webhook_secret_name();

        match secret_api.get(&name).await {
            Ok(secret) => Ok(AdmissionTls::from(&secret).unwrap()),
            Err(e) => Err(anyhow!(e)),
        }
    }
}
