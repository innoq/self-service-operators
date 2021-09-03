use std::sync::Arc;

use crate::project::Project;
use krator::{Manifest, State, Transition, TransitionTo};
use tokio::sync::RwLock;

use crate::helper;
use crate::project::ProjectStatus;
use crate::self_service::transitions::{ProjectPhase, ProjectState, SharedState};

#[derive(Debug, Default)]
/// Something went wrong
pub struct Error;

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
