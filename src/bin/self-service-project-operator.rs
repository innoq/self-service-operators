#[macro_use]
extern crate log;

use std::time::Duration;
use std::{convert::TryFrom, process::exit};

use anyhow::Context;
use clap::{crate_authors, crate_version, Clap};
use env_logger::*;
use krator::OperatorRuntime;
use log::LevelFilter;
pub use schemars::JsonSchema;

use noqnoqnoq::self_service::operator;
use noqnoqnoq::self_service::project;
use noqnoqnoq::self_service::project::Project;
use noqnoqnoq::self_service::project::Sample;

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

    debug!("logging level set to 'debug' -- don't use this in production as it can pontentially leak sensitive information");

    if opts.print_crd {
        println!(
            "# self service crd (auto-generated with 'noqnoqnoq --print-crd'):\n{}\n",
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

    let kubeconfig = kube::config::Config::infer().await?;

    let namespace = match opts.namespace {
        Some(ref namespace) => namespace,
        None => &kubeconfig.default_ns,
    };

    if opts.print_admission_controller_manifests {
        println!(
            "{}",
            krator::admission::WebhookResources::from(Project::admission_webhook_resources(
                &namespace
            ))
        );

        exit(0)
    }

    let client = kube::Client::try_from(kubeconfig.clone())
        .context("error creating kubernetes client from the current environment")?;

    if opts.install_crd {
        return noqnoqnoq::install_crd(&client, &Project::crd())
            .await
            .and(Ok(()));
    }

    if !opts.skip_install_admission_controller_manifests {
        let resources = krator::admission::WebhookResources::from(
            Project::admission_webhook_resources(&namespace),
        );

        resources.apply(&client).await?;
    }

    let tracker = operator::ProjectOperator::new(
        client,
        &opts.default_owner_cluster_role,
        &namespace,
        crate::project::DEFAULT_MANIFESTS_SECRET,
        Duration::from_secs(5),
    )
    .await?;

    // Only track mooses in Glacier NP
    // let params = ListParams::default().labels("nps.gov/park=glacier");
    let mut runtime = OperatorRuntime::new(&kubeconfig, tracker, None);
    runtime.start().await;
    Ok(())
}
