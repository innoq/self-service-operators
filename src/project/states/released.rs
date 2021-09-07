use std::sync::Arc;

use krator::{Manifest, State, Transition};
use tokio::sync::RwLock;

use crate::project::project_status::ProjectStatus;
use crate::project::states::{ProjectState, SharedState};
use crate::project::Project;

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
