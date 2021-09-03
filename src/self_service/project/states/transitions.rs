use krator::TransitionTo;

use crate::self_service::project::states::{
    ApplyManifests, CreateNamespace, Error, SetupRBACPermissions, WaitForChanges,
};

impl TransitionTo<SetupRBACPermissions> for CreateNamespace {}
impl TransitionTo<Error> for CreateNamespace {}

impl TransitionTo<ApplyManifests> for SetupRBACPermissions {}
impl TransitionTo<Error> for SetupRBACPermissions {}

impl TransitionTo<WaitForChanges> for ApplyManifests {}
impl TransitionTo<Error> for ApplyManifests {}

impl TransitionTo<WaitForChanges> for WaitForChanges {}
impl TransitionTo<CreateNamespace> for WaitForChanges {}
impl TransitionTo<Error> for WaitForChanges {}

impl TransitionTo<Error> for Error {}
