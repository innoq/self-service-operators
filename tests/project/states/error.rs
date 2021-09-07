use std::collections::BTreeMap;
use std::time::Duration;

use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::PostParams;
use serial_test::serial;
use tokio::time;

use self_service_operators::project::{Project, ProjectSpec, Sample};

use crate::{project};

#[tokio::test]
#[serial]
async fn it_fails_with_correct_error_state_when_invalid_manifests_are_used() -> anyhow::Result<()> {
    let (client, _) = project::before_each().await?;

    let name = project::random_name("fail-with-invalid-manifests");

    project::apply_manifest_secret(
        &client,
        "extra-manifests",
        vec![include_str!("../../fixtures/invalid.yaml")],
    )
    .await?;

    let manifest_values = "name: standard-pod";

    let mut spec = ProjectSpec::sample();
    spec.manifest_values = Some(manifest_values.into());

    let mut annotations = BTreeMap::new();
    annotations.insert(
        "project.selfservice.innoq.io/extra-manifests".to_string(),
        "copy".to_string(),
    );

    let project = Project {
        metadata: ObjectMeta {
            name: Some(name.clone()),
            annotations: Some(annotations),
            ..Default::default()
        },
        spec,
        ..Default::default()
    };

    let api: kube::Api<Project> = kube::Api::all(client.clone());
    let _ = api.create(&PostParams::default(), &project).await?;

    let mut last_summary = "".to_string();
    for _ in 0..20 {
        let _ = time::sleep(Duration::from_secs(1)).await;
        let project = api.get(&name).await?;
        if project.status.clone().unwrap().summary.is_none() {
            continue;
        }

        let status = &project.status.clone().unwrap();
        last_summary = status.summary.clone().unwrap();
        if last_summary == *"error: error installing manifest: giving up aft..." {
            return Ok(());
        }
    }

    panic!(
        "correct error state never reached -- last error summary was: {}",
        last_summary
    );
}
