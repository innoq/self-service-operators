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

use anyhow::{Context};
use clap::{crate_authors, crate_version, Clap};
use env_logger::*;
use k8s_openapi::api::core::v1::Secret;

use log::{debug, info, LevelFilter};
pub use schemars::JsonSchema;



use self_service_operators::project::Project;
use self_service_operators::project::Sample;

#[derive(Clap)]
#[clap(
version = crate_version!(),
author = crate_authors!()
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

    /// Test manifest template:outputs the result of a given manifest template / project combination: expects <PROJECT.YAML>,<MANIFEST.YAML> (files separated by comma)
    #[clap(short = 't', long)]
    test_manifest_template: Option<String>,

    /// verbose level
    #[clap(short = 'd', long, default_value = "db-connection")]
    db_connection_secret: String,

    /// verbose level
    #[clap(short, long, default_value = "info", possible_values = &["debug", "info", "warn", "error"]) ]
    verbosity_level: String,
}

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

    let client = kube::Client::try_from(kubeconfig.clone())
        .context("error creating kubernetes client from the current environment")?;

    if opts.install_crd {
        info!("installing crd");
        return self_service_operators::install_crd(&client, &Project::crd())
            .await
            .and(Ok(()));
    }

    let _api: kube::Api<Secret> = kube::Api::namespaced(client, &kubeconfig.default_ns);

    // let tracker = operator::PostgresDbOperator::new(client, &kubeconfig.default_ns).await?;

    // info!("starting operator");
    // // let params = ListParams::default().labels("nps.gov/park=glacier");
    // let mut runtime = OperatorRuntime::new(&kubeconfig, tracker, None);
    // runtime.start().await;
    Ok(())
}
