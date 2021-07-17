#[macro_use]
extern crate log;

use std::{convert::TryFrom, process::exit};

use clap::{crate_authors, crate_version, Clap};
use env_logger::*;
use krator::OperatorRuntime;
use log::LevelFilter;
pub use schemars::JsonSchema;

use anyhow::Context;
use noqnoqnoq::self_service::helper;
use noqnoqnoq::self_service::project;
use noqnoqnoq::self_service::Sample;

#[derive(Clap)]
#[clap(
  version = crate_version!(),
  author = crate_authors!()
)]
struct Opts {
    /// Install self service crd into cluster
    #[clap(short = 'i', long)]
    install_crd: bool,

    /// Prints the self service crd to stdout
    #[clap(short, long)]
    print_crd: bool,

    /// Prints an self service project sample manifest
    #[clap(short = 's', long)]
    print_sample_project_manifest: bool,

    /// verbose level
    #[clap(short, long, default_value = "info", possible_values = &["debug", "info", "warn", "error"]) ]
    verbosity_level: String,

    /// cluster role the owner of this project should get in the project namespace
    #[clap(short, long, default_value = "admin")]
    default_owner_cluster_role: String,
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
        .filter(Some("noqnoqnoq"), level)
        .filter(Some("self_service_project_operator"), level)
        .init();

    debug!("logging level set to 'debug' -- don't use this in production as it can pontentially leak sensible information");

    if opts.print_crd {
        println!(
            "# self service crd (auto-generated with 'noqnoqnoq --print-crd'):\n{}\n",
            serde_yaml::to_string(&project::Project::crd()).unwrap()
        );
        exit(0)
    }

    if opts.print_sample_project_manifest {
        println!(
      "# self service sample project manifest (auto-generated with 'self-service-operator --print-sample-project-manifest'):\n{}\n",
      serde_yaml::to_string(&project::Project::sample()).unwrap()
    );
        exit(0)
    }

    let kubeconfig = kube::config::Config::infer().await?;

    let client = kube::Client::try_from(kubeconfig.clone())
        .context("error creating kubernetes client from the current environment")?;

    if opts.install_crd {
        return helper::install_crd(&client, &project::Project::crd())
            .await
            .and(Ok(()));
    }

    let tracker = project::ProjectOperator::new(
        client,
        &opts.default_owner_cluster_role,
        &kubeconfig.default_ns,
        crate::project::DEFAULT_MANIFESTS_SECRET,
    )
    .await?;

    // Only track mooses in Glacier NP
    // let params = ListParams::default().labels("nps.gov/park=glacier");
    let mut runtime = OperatorRuntime::new(&kubeconfig, tracker, None);
    runtime.start().await;
    Ok(())
}
