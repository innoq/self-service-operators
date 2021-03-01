use std::cmp::PartialEq;
use std::sync::Arc;

use k8s_openapi::Metadata;
use krator::{Manifest, ObjectState, ObjectStatus, Operator, State, Transition, TransitionTo};
use kube::CustomResource;
// use kube::api::ListParams;
use super::Sample;
use crate::project::ProjectPhase::Initializing;
use futures::StreamExt;
use k8s_openapi::api::core::v1::Namespace;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::api::{ObjectMeta, PostParams};
pub use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tokio::sync::RwLock;

// TODO: follow up on https://github.com/clux/kube-rs/issues/264#issuecomment-748327959
#[derive(CustomResource, Serialize, Deserialize, PartialEq, Default, Debug, Clone, JsonSchema)]
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

impl Sample for ProjectSpec {
    fn sample() -> Self {
        ProjectSpec {
            owner: "superdev@example.com".to_string(),
            private: Some(false),
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
            "project.selfservice.innoq.io/gitlabci-container-registry-secrets.public-key"
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
    fn failed(e: &str) -> ProjectStatus {
        ProjectStatus {
            message: Some(format!("Error occured: {}", e)),
            phase: None,
        }
    }

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
}

pub struct ProjectState {
    name: String,
    spec: ProjectSpec,
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
        let manifest = manifest.latest();

        let namespace = Namespace {
            metadata: ObjectMeta {
                name: Some(state.name.clone()),
                owner_references: Some(vec![OwnerReference {
                    api_version: manifest.api_version,
                    block_owner_deletion: None,
                    controller: None,
                    kind: manifest.kind,
                    name: state.name.clone(),
                    uid: manifest.metadata.uid.unwrap(),
                }]),
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

#[async_trait::async_trait]
impl State<ProjectState> for SetupRBACPermissions {
    async fn next(
        self: Box<Self>,
        _shared: Arc<RwLock<SharedState>>,
        state: &mut ProjectState,
        _manifest: Manifest<Project>,
    ) -> Transition<ProjectState> {
        debug!("next() in SetupRBACPermissions / name: {}", state.name);
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

#[async_trait::async_trait]
impl State<ProjectState> for WaitForChanges {
    async fn next(
        self: Box<Self>,
        _shared: Arc<RwLock<SharedState>>,
        _state: &mut ProjectState,
        manifest: Manifest<Project>,
    ) -> Transition<ProjectState> {
        let mut x = manifest.clone();
        while let Some(current) = x.next().await {
            info!(
                "manifest for {} was updated",
                current.metadata.name.unwrap()
            );
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
}

impl SharedState {
    pub fn client(&self) -> kube::Client {
        self.client.clone()
    }
}

pub struct ProjectOperator {
    shared: Arc<RwLock<SharedState>>,
}

impl ProjectOperator {
    pub fn new(client: kube::Client) -> Self {
        let shared = Arc::new(RwLock::new(SharedState { client: client }));
        ProjectOperator { shared }
    }
}

#[async_trait::async_trait]
impl Operator for ProjectOperator {
    type Manifest = Project;
    type Status = ProjectStatus;
    type InitialState = NewProject;
    type DeletedState = Released;
    type ObjectState = ProjectState;

    async fn initialize_object_state(
        &self,
        manifest: &Self::Manifest,
    ) -> anyhow::Result<Self::ObjectState> {
        let name = manifest.metadata().name.clone().unwrap();
        let spec = manifest.spec.clone();
        Ok(ProjectState {
            name: name,
            spec: spec,
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
}
