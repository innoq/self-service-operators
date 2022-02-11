/*
 * Copyright 2021 Daniel Bornkessel
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use std::sync::Arc;

use crate::postgres::operator::PostgresOperatorState;
use krator::{Manifest, State, Transition};
use tokio::sync::RwLock;

use crate::postgres::postgres_status::PostgresStatus;
use crate::postgres::states::PostgresState;
use crate::postgres::Postgres;

#[derive(Debug, Default)]
/// Postgres is sleeping.
pub(crate) struct WaitForChanges;

#[async_trait::async_trait]
impl State<PostgresState> for WaitForChanges {
    async fn next(
        self: Box<Self>,
        _shared: Arc<RwLock<PostgresOperatorState>>,
        _state: &mut PostgresState,
        _manifest: Manifest<Postgres>,
    ) -> Transition<PostgresState> {
        unimplemented!();
        // let lp = &ListParams::default().fields(&format!("metadata.name={}", state.name));
        // let mut stream = kube::Api::<Postgres>::all(shared.read().await.client.clone())
        //     .watch(lp, &(manifest.latest().metadata.resource_version.unwrap()))
        //     .await
        //     .unwrap()
        //     .boxed();
        //
        // match stream.try_next().await {
        //     Ok(Some(status)) => match status.clone() {
        //         WatchEvent::Modified(_resource) => {
        //             info!("postgres {} modified", state.name);
        //             return Transition::next(self, CreateNamespace);
        //         }
        //         WatchEvent::Error(e) => {
        //             warn!(
        //                 "ERROR watching Postgres with name {}: {}",
        //                 state.name, e.message
        //             );
        //         }
        //         _ => debug!(
        //             "unimplemented state while watching for changes on Postgres with name {}: {:?}",
        //             state.name, status
        //         ),
        //     },
        //     Err(e) => {
        //         state.error = e.to_string();
        //         return Transition::next(self, Error);
        //     }
        //     _ => {
        //         print!("#");
        //     }
        // }
        //
        // Transition::next(self, WaitForChanges)
    }

    async fn status(
        &self,
        _state: &mut PostgresState,
        _postgres: &Postgres,
    ) -> anyhow::Result<PostgresStatus> {
        debug!("status() in WaitForChanges");
        unimplemented!();
        // Ok(PostgresStatus {
        //     phase: Some(PostgresPhase::WaitingForChanges),
        //     message: Some("waiting for changes".to_string()),
        //     summary: Some("waiting for changes".to_string()),
        //     applied_one_shot_resources: postgres
        //         .status
        //         .clone()
        //         .unwrap_or_else(PostgresStatus::default)
        //         .applied_one_shot_resources,
        // })
    }
}
