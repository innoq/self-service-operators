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

use krator::admission::{AdmissionResult, AdmissionTls};
use krator::{Manifest, Operator};
use tokio::sync::RwLock;

use crate::postgres::postgres_status::PostgresStatus;
use crate::postgres::states::{CreateDatabase, PostgresState, Released};
use crate::postgres::Postgres;

#[derive(Clone)]
pub struct PostgresOperator {
    shared: Arc<RwLock<PostgresOperatorState>>,
}

impl PostgresOperator {
    pub async fn new(
        kube_client: kube::Client,
        postgres_client: tokio_postgres::Client,
    ) -> anyhow::Result<Self> {
        let shared = Arc::new(RwLock::new(PostgresOperatorState {
            _kube_client: kube_client.clone(),
            _postgres_client: postgres_client,
        }));

        Ok(PostgresOperator { shared })
    }
}

#[async_trait::async_trait]
impl Operator for PostgresOperator {
    type Manifest = Postgres;
    type Status = PostgresStatus;
    type ObjectState = PostgresState;
    type InitialState = CreateDatabase;
    type DeletedState = Released;

    async fn initialize_object_state(
        &self,
        _manifest: &Self::Manifest,
    ) -> anyhow::Result<Self::ObjectState> {
        unimplemented!();
        // let name = manifest.meta().name.clone().unwrap();
        // Ok(PostgresState {
        //     name,
        //     error: "".to_string(),
        //     applied_one_shot_resources: HashSet::new(),
        // })
    }

    async fn shared_state(&self) -> Arc<RwLock<PostgresOperatorState>> {
        Arc::clone(&self.shared)
    }

    async fn registration_hook(&self, manifest: Manifest<Self::Manifest>) -> anyhow::Result<()> {
        warn!(
            "REGISTRATION HOOK CALLED {}",
            serde_yaml::to_string(&manifest.latest()).unwrap()
        );
        Ok(())
    }

    async fn admission_hook(&self, _postgres: Self::Manifest) -> AdmissionResult<Self::Manifest> {
        unimplemented!("TODO");
        // let shared = self.shared.read().await;
        // let client = shared.client.clone();
        // let default_namespace = shared.default_ns.clone();
        // let postgres_name = postgres.metadata.name.as_ref().expect("");
        //
        // debug!("admission hook: {:?}", postgres);
        //
        // let deny = |msg: String| {
        //     AdmissionResult::Deny(Status {
        //         code: Some(409),
        //         details: None,
        //         message: Some(msg),
        //         metadata: ListMeta {
        //             ..Default::default()
        //         },
        //
        //         reason: None,
        //         status: Some("Failure".to_string()),
        //     })
        // };
        //
        // if let Ok(postgres_namespace) = Api::<Namespace>::all(client.clone())
        //     .get(&postgres_name)
        //     .await
        // {
        //     if let Some(owner_references) = postgres_namespace.metadata.owner_references {
        //         let ns_owned_by_this_postgres =
        //             owner_references.into_iter().any(|owner_reference| {
        //                 owner_reference.kind == Postgres::kind(&())
        //                     && owner_reference.name == *postgres_name
        //             });
        //
        //         if !ns_owned_by_this_postgres {
        //             return deny(format!(
        //                 "can't create/update postgres: a namespace with name '{}' already exists but is not owned by this postgres",
        //                 postgres_name
        //             ));
        //         }
        //     } else {
        //         return deny(format!(
        //             "can't create postgres: a namespace with name '{}' already exists",
        //             postgres_name
        //         ));
        //     }
        // }
        //
        // if let Err(e) = postgres
        //     .associated_manifests(
        //         &client,
        //         &shared.default_manifests_secret,
        //         &default_namespace,
        //     )
        //     .await
        // {
        //     return deny(e.to_string());
        // }
        //
        // AdmissionResult::Allow(postgres)
    }

    async fn admission_hook_tls(&self) -> anyhow::Result<AdmissionTls> {
        unimplemented!();
        // let client = self.shared.read().await.kube_client.clone();
        // let namespace = &self.shared.read().await.default_ns;
        //
        // let name = Postgres::admission_webhook_secret_name();
        //
        // debug!(
        //     "reading admission webhook certificates from secret {}/{}",
        //     &namespace, &name
        // );
        //
        // match Api::<Secret>::namespaced(client, namespace)
        //     .get(&name)
        //     .await
        // {
        //     Ok(secret) => Ok(AdmissionTls::from(&secret).unwrap()),
        //     Err(e) => Err(anyhow!(e)),
        // }
    }

    async fn deregistration_hook(
        &self,
        mut _manifest: Manifest<Self::Manifest>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

pub struct PostgresOperatorState {
    pub(crate) _kube_client: kube::Client,
    pub(crate) _postgres_client: tokio_postgres::Client,
}
