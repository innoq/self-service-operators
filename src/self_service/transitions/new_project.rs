use crate::project::Project;
use std::sync::Arc;

use krator::{Manifest, State, Transition, TransitionTo};
use tokio::sync::RwLock;

use crate::project::ProjectStatus;
use crate::self_service::transitions::ProjectPhase::Initializing;
use crate::self_service::transitions::{CreateNamespace, ProjectState, SharedState};

#[derive(Debug, Default)]
/// New project was detected
// TODO: weg damit
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
