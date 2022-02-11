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
/// Postgres was released from our care.
pub struct Released;

#[async_trait::async_trait]
impl State<PostgresState> for Released {
    async fn next(
        self: Box<Self>,
        _shared: Arc<RwLock<PostgresOperatorState>>,
        state: &mut PostgresState,
        _manifest: Manifest<Postgres>,
    ) -> Transition<PostgresState> {
        debug!("next() in Released / name: {}", state.name);
        Transition::Complete(Ok(()))
    }

    async fn status(
        &self,
        _state: &mut PostgresState,
        _postgres: &Postgres,
    ) -> anyhow::Result<PostgresStatus> {
        Ok(PostgresStatus {
            summary: None,
            deletion_protection: false,
        })
    }
}
