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
/// Postgres is creating a namespace
pub struct CreateDatabase;

#[async_trait::async_trait]
impl State<PostgresState> for CreateDatabase {
    async fn next(
        self: Box<Self>,
        _shared: Arc<RwLock<PostgresOperatorState>>,
        state: &mut PostgresState,
        _manifest: Manifest<Postgres>,
    ) -> Transition<PostgresState> {
        info!("creating database {}", &state.name);
        unimplemented!("missing");
        //
        // let api: kube::Api<Namespace> = kube::Api::all(shared.read().await.client.clone());
        // let postgres = manifest.latest();
        // let name = postgres.clone().metadata.name.unwrap();
        //
        // if let Ok(namespace) = api.get(&name).await {
        //     if is_owned_by_postgres(&postgres, &namespace) {
        //         return Transition::next(self, ApplyManifests);
        //     } else {
        //         state.error = format!(
        //             "namespace '{}' exists but does not belong to postgres '{}'",
        //             &name, &name
        //         );
        //         return Transition::next(self, Error);
        //     }
        // }
        //
        // let namespace = Namespace {
        //     metadata: ObjectMeta {
        //         name: Some(name),
        //         owner_references: Some(vec![OwnerReference::from(&postgres)]),
        //         ..Default::default()
        //     },
        //     ..Default::default()
        // };
        //
        // if let Err(e) = api.create(&PostParams::default(), &namespace).await {
        //     state.error = format!("error creating namespace {}: {}", state.name, e.to_string());
        //     Transition::next(self, Error)
        // } else {
        //     Transition::next(self, ApplyManifests)
        // }
    }

    async fn status(
        &self,
        _state: &mut PostgresState,
        _postgres: &Postgres,
    ) -> anyhow::Result<PostgresStatus> {
        debug!("status() in CreateNamespace");
        unimplemented!("status");
        // Ok(PostgresStatus {
        //     phase: Some(PostgresPhase::CreatingNamespace),
        //     message: Some(format!("creating namespace {}", state.name)),
        //     summary: Some(format!("creating namespace {}", state.name)),
        //     applied_one_shot_resources: postgres
        //         .status
        //         .clone()
        //         .unwrap_or_else(PostgresStatus::default)
        //         .applied_one_shot_resources,
        // })
    }
}
