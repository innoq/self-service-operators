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
/// Something went wrong
pub struct Error;

#[async_trait::async_trait]
impl State<PostgresState> for Error {
    async fn next(
        self: Box<Self>,
        _shared: Arc<RwLock<PostgresOperatorState>>,
        state: &mut PostgresState,
        _manifest: Manifest<Postgres>,
    ) -> Transition<PostgresState> {
        info!("error {}", &state.name);
        unimplemented!();

        // let lp = &ListParams::default().fields(&format!("metadata.name={}", state.name));
        // let mut stream = kube::Api::<Postgres>::all(shared.read().await.kube_client.clone())
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
        //         let timeout = Duration::from_secs(60);
        //         state.error = e.to_string();
        //         warn!("error watching stream for new events: {} ... waiting {} seconds in order to avoid log flooding", &state.error, &timeout.as_secs());
        //         tokio::time::sleep(timeout).await;
        //         return Transition::next(self, Error);
        //     }
        //     _ => {
        //         print!("#");
        //     }
        // }
        //
        // Transition::next(self, Error)
    }

    async fn status(
        &self,
        _state: &mut PostgresState,
        _postgres: &Postgres,
    ) -> anyhow::Result<PostgresStatus> {
        debug!("status() in Error");
        unimplemented!();
        // let message = format!("error: {}", state.error);
        // Ok(PostgresStatus {
        //     phase: Some(PostgresPhase::FailedDueToError),
        //     summary: Some(crate::postgres::shorten_string(&message)),
        //     message: Some(message),
        //     applied_one_shot_resources: postgres
        //         .status
        //         .clone()
        //         .unwrap_or_else(PostgresStatus::default)
        //         .applied_one_shot_resources,
        // })
    }
}
