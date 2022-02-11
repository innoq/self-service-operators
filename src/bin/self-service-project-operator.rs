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

use std::fs::File;
use std::io::Read;
use std::time::Duration;
use std::{convert::TryFrom, process::exit};

use anyhow::{bail, Context};
use clap::{crate_authors, crate_version, Clap};
use env_logger::*;
use krator::OperatorRuntime;
use log::{debug, info, LevelFilter};
pub use schemars::JsonSchema;

use self_service_operators::project::operator;
use self_service_operators::project::project::DEFAULT_MANIFESTS_SECRET;
use self_service_operators::project::Project;
use self_service_operators::project::Sample;

#[derive(Clap)]
#[clap(
  version = crate_version!(),
  author = crate_authors!()
)]
struct Opts {
    /// Prints the self service crd to stdout
    #[clap(short = 'c', long)]
    print_crd: bool,

    /// Install self service crd into cluster
    #[clap(short = 'C', long)]
    install_crd: bool,

    /// Prints necessary resources to setup admission controller. Uses current namespaces unless set by --namespace
    #[clap(short = 'a', long)]
    print_admission_controller_manifests: bool,

    /// Namespace to use when printing / installing admission controller and default namespacec
    /// for manifest secrets
    #[clap(short = 'n', long)]
    namespace: Option<String>,

    /// Skip installation of webhook resources
    #[clap(short = 'A', long)]
    skip_install_admission_controller_manifests: bool,

    /// Prints an self service project sample manifest
    #[clap(short = 'm', long)]
    print_sample_project_manifest: bool,

    /// Test manifest template:outputs the result of a given manifest template / project combination: expects <PROJECT.YAML>,<MANIFEST.YAML> (files separated by comma)
    #[clap(short = 't', long)]
    test_manifest_template: Option<String>,

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
        .filter(Some("self_service_project_operator"), level)
        .filter(Some("self_service_operators::project"), level)
        .init();

    info!(
        "starting Self Service Project operator version {}",
        crate_version!()
    );
    debug!("logging level set to 'debug' -- don't use this in production as it can pontentially leak sensitive information");

    if opts.print_crd {
        println!(
            "# self service crd (auto-generated):\n{}\n",
            serde_yaml::to_string(&Project::crd()).unwrap()
        );
        exit(0)
    }

    if opts.print_sample_project_manifest {
        println!(
      "# self service sample project manifest (auto-generated with 'self-service-operator --print-sample-project-manifest'):\n{}\n",
      serde_yaml::to_string(&Project::sample()).unwrap()
    );
        exit(0)
    }

    if let Some(files) = opts.test_manifest_template {
        let filenames: Vec<&str> = files.split(',').collect();
        if filenames.len() != 2 {
            bail!("in order to check the templating of a manifest file, pass in a project resource yaml file and a template file, separated by a comman, e.g. --test-manifest-template project.yaml,manifest.yaml");
        }

        let project: Project = serde_yaml::from_reader(File::open(filenames[0])?)?;

        let mut manifest_file = String::new();
        File::open(filenames[1])?.read_to_string(&mut manifest_file)?;

        let rendered_file = project.render(&manifest_file, filenames[1])?;

        println!("{}", rendered_file);

        exit(0)
    }

    debug!("infering kubernetes config");
    let kubeconfig = kube::config::Config::infer().await?;

    let namespace = match opts.namespace {
        Some(ref namespace) => namespace,
        None => &kubeconfig.default_ns,
    };

    info!("using namespace {}", namespace);

    if opts.print_admission_controller_manifests {
        println!(
            "{}",
            krator::admission::WebhookResources::from(Project::admission_webhook_resources(
                namespace
            ))
        );

        exit(0)
    }

    let client = kube::Client::try_from(kubeconfig.clone())
        .context("error creating kubernetes client from the current environment")?;

    if opts.install_crd {
        info!("installing crd");
        return self_service_operators::install_crd(&client, &Project::crd())
            .await
            .and(Ok(()));
    }

    if !opts.skip_install_admission_controller_manifests {
        info!("installing admission controller resources");
        let resources = krator::admission::WebhookResources::from(
            Project::admission_webhook_resources(namespace),
        );

        resources.apply(&client).await?;
    }

    let tracker = operator::ProjectOperator::new(
        client,
        namespace,
        DEFAULT_MANIFESTS_SECRET,
        Duration::from_secs(5),
    )
    .await?;

    info!("starting operator");
    // let params = ListParams::default().labels("nps.gov/park=glacier");
    let mut runtime = OperatorRuntime::new(&kubeconfig, tracker, None);
    runtime.start().await;
    Ok(())
}
