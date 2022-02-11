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

use std::{convert::TryFrom, process::exit};

use anyhow::{anyhow, Context};
use clap::{crate_authors, crate_version, Clap};
use env_logger::*;
use k8s_openapi::api::core::v1::Secret;
use krator::OperatorRuntime;

use crate::postgres::Postgres;
use log::{debug, info, LevelFilter};
pub use schemars::JsonSchema;
use self_service_operators::postgres;
use tokio_postgres::NoTls;

use self_service_operators::project::Project;
use self_service_operators::project::Sample;

#[derive(Clap)]
#[clap(
version = crate_version ! (),
author = crate_authors ! ()
)]
struct Opts {
    /// Prints the postgresdb crd to stdout
    #[clap(short = 'c', long)]
    print_crd: bool,

    /// Install postgresdb crd into cluster
    #[clap(short = 'C', long)]
    install_crd: bool,

    /// Prints an postgresdb project sample manifest
    #[clap(short = 'm', long)]
    print_sample_postgresdb_manifest: bool,

    /// Skip installation of webhook resources
    #[clap(short = 'A', long)]
    skip_install_admission_controller_manifests: bool,

    /// verbose level
    #[clap(short = 'd', long, default_value = "db-connection")]
    db_connection_secret: String,

    /// verbose level
    #[clap(short, long, default_value = "info", possible_values = &["debug", "info", "warn", "error"])]
    verbosity_level: String,
}

const CONNECTION_STRING_VARIABLE: &str = "connection_string";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts: Opts = Opts::parse();
    let mut builder = Builder::from_default_env();

    let level = match opts.verbosity_level.as_str() {
        "debug" => LevelFilter::Debug,
        "info" => LevelFilter::Info,
        "warn" => LevelFilter::Warn,
        "error" => LevelFilter::Error,
        _ => unreachable!(), // guarded by clap / getops config further up
    };

    builder
        .filter(Some("self_service_postgresdb_operator"), level)
        .filter(Some("self_service_operators::postgresdb"), level)
        .init();

    info!(
        "starting postgresdb Project operator version {}",
        crate_version!()
    );

    debug!("logging level set to 'debug' -- don't use this in production as it can potentially leak sensitive information");

    if opts.print_crd {
        println!(
            "# postgresdb crd (auto-generated):\n{}\n",
            serde_yaml::to_string(&Project::crd()).unwrap()
        );
        exit(0)
    }

    if opts.print_sample_postgresdb_manifest {
        println!(
            "# postgresdb sample project manifest (auto-generated with 'self-service-operator --print-sample-project-manifest'):\n{}\n",
            serde_yaml::to_string(&Project::sample()).unwrap()
        );
        exit(0)
    }

    debug!("infering kubernetes config");
    let kubeconfig = kube::config::Config::infer().await?;

    let kube_client = kube::Client::try_from(kubeconfig.clone())
        .context("error creating kubernetes client from the current environment")?;

    if opts.install_crd {
        info!("installing crd");
        return self_service_operators::install_crd(&kube_client, &Project::crd())
            .await
            .and(Ok(()));
    }

    let namespace = kubeconfig.default_ns.as_str();
    if !opts.skip_install_admission_controller_manifests {
        info!("installing admission controller resources");
        let resources = krator::admission::WebhookResources::from(
            Postgres::admission_webhook_resources(namespace),
        );

        resources.apply(&kube_client).await?;
    }

    let api: kube::Api<Secret> = kube::Api::namespaced(kube_client.clone(), namespace);
    let secret = api
        .get(&opts.db_connection_secret)
        .await
        .context(format!("Secret {} not found.", &opts.db_connection_secret))?;

    let connection_string = String::from_utf8(
        secret
            .data
            .ok_or_else(|| anyhow!("Secret {} is empty", &opts.db_connection_secret))?
            .get(CONNECTION_STRING_VARIABLE)
            .context(format!(
                "Secret {} doesn't have a key with name {}",
                &opts.db_connection_secret, CONNECTION_STRING_VARIABLE
            ))?
            .to_owned()
            .0,
    )?;

    // TODO tls
    debug!("Trying to connect to database");
    let (postgres_client, _) = tokio_postgres::connect(&connection_string, NoTls)
        .await
        .context(format!(
            "Couldn't connect to database using connection string found in secret {}/{}",
            &opts.db_connection_secret, CONNECTION_STRING_VARIABLE
        ))?;
    info!("Database connection successful!");
    info!("starting operator");

    let operator = postgres::operator::PostgresOperator::new(
        kube_client,
        postgres_client,
        &kubeconfig.default_ns,
    )
    .await?;
    // let params = ListParams::default().labels("nps.gov/park=glacier");
    let mut runtime = OperatorRuntime::new(&kubeconfig, operator, None);
    runtime.start().await;

    // CREATE ROLE $DBNAME NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOLOGIN;
    // CREATE ROLE $DBMAINUSER NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT LOGIN ENCRYPTED PASSWORD 'foopass';
    // GRANT $DBNAME TO $DBMAINUSER;
    // grant $DBMAINUSER to postgres;
    // CREATE DATABASE $DBNAME WITH OWNER=$DBMAINUSER;
    // REVOKE ALL ON DATABASE $DBNAME FROM public;

    // let tracker = operator::PostgresDbOperator::new(client, &kubeconfig.default_ns).await?;

    // info!("starting operator");
    // // let params = ListParams::default().labels("nps.gov/park=glacier");
    // let mut runtime = OperatorRuntime::new(&kubeconfig, tracker, None);
    // runtime.start().await;
    Ok(())
}
