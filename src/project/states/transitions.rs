use krator::TransitionTo;

use crate::project::states::{ApplyManifests, CreateNamespace, Error, WaitForChanges};

impl TransitionTo<ApplyManifests> for CreateNamespace {}
impl TransitionTo<Error> for CreateNamespace {}

impl TransitionTo<WaitForChanges> for ApplyManifests {}
impl TransitionTo<Error> for ApplyManifests {}

impl TransitionTo<WaitForChanges> for WaitForChanges {}
impl TransitionTo<CreateNamespace> for WaitForChanges {}
impl TransitionTo<Error> for WaitForChanges {}

impl TransitionTo<Error> for Error {}
