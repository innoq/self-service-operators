use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::ensure;
use k8s_openapi::api::core::v1::{Namespace, Secret};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ListMeta, Status};
use krator::admission::{AdmissionResult, AdmissionTls};
use krator::{Manifest, Operator};
use kube::{Api, Resource};
use tokio::sync::RwLock;

use crate::project::project::{SECRET_ANNOTATION_KEY, SECRET_ANNOTATION_VALUE};
use crate::project::project_status::ProjectStatus;
use crate::project::states::{CreateNamespace, ProjectState, Released, SharedState};
use crate::project::Project;

#[derive(Clone)]
pub struct ProjectOperator {
    shared: Arc<RwLock<SharedState>>,
}

impl ProjectOperator {
    pub async fn new(
        client: kube::Client,
        default_ns: &str,
        default_manifests_secret: &str,
        manifest_retry_delay: Duration,
    ) -> anyhow::Result<Self> {
        let shared = Arc::new(RwLock::new(SharedState {
            client: client.clone(),
            default_ns: default_ns.to_string(),
            default_manifests_secret: default_manifests_secret.to_string(),
            manifest_retry_delay,
        }));

        if let Err(e) = get_manifests_secret(&client, default_manifests_secret, default_ns).await {
            bail!(
                    "no Secret with name '{}' in namespace '{}' found (this secret should hold default manifests that get applied in each new namespace): {} -- aborting",
                    default_manifests_secret, default_ns, e);
        }

        Ok(ProjectOperator { shared })
    }
}

#[async_trait::async_trait]
impl Operator for ProjectOperator {
    type Manifest = Project;
    type Status = ProjectStatus;
    type ObjectState = ProjectState;
    type InitialState = CreateNamespace;
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
        let client = self.shared.read().await.client.clone();
        let namespace = &self.shared.read().await.default_ns;

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
        .and_then(|annotations| annotations.get(SECRET_ANNOTATION_KEY));

    ensure!(
        annotation.is_some() && annotation.unwrap() == SECRET_ANNOTATION_VALUE,
        "Error accessing secret '{}': only secrets with the annotation '{}: {}' can be accessed by the project operator",
        secret_name,
        SECRET_ANNOTATION_KEY,
        SECRET_ANNOTATION_VALUE
        );

    Ok(secret)
}
