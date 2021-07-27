use std::collections::BTreeMap;

use krator::{admission::AdmissionResult, Operator};
use kube::Resource;
use noqnoqnoq::project::{self, Project};
use serial_test::serial;

mod common;

#[tokio::test]
#[serial]
async fn it_should_be_possible_to_update_projects() -> anyhow::Result<()> {
    let (client, operator) = common::before_each().await?;

    let name = common::random_name("update-project-test");
    let _ = common::install_project(&client, &name).await?;

    let project = Project::new(&name, Default::default());

    let result = operator.admission_hook(project).await;

    match result {
        AdmissionResult::Allow(_) => {}
        _ => panic!("admission hook should pass when a project gets updated"),
    }
    Ok(())
}

#[tokio::test]
#[serial]
async fn it_should_fail_if_namespace_already_exists_but_was_not_created_by_this_operator(
) -> anyhow::Result<()> {
    let (_, operator) = common::before_each().await?;

    let project = Project::new("default", Default::default());

    let result = operator.admission_hook(project).await;

    match result {
        AdmissionResult::Deny(status) => {
            assert_eq!(status.code, Some(409));
            assert_eq!(
                status.message,
                Some(
                    "can't create project: a namespace with name 'default' already exists"
                        .to_string()
                )
            );
            assert_eq!(status.status, Some("Failure".to_string()));
        }
        _ => panic!(
            "admission hook did not fail if a new project's name clashes with an existing namespace"
        ),
    }

    Ok(())
}

#[tokio::test]
#[serial]
async fn it_should_fail_if_secret_with_resource_manifests_is_not_available() -> anyhow::Result<()> {
    let (_, operator) = common::before_each().await?;

    let name = common::random_name("missing-secret");
    let mut project = Project::new(&name, Default::default());

    let mut meta_data = project.meta_mut();

    let mut annotations = BTreeMap::new();
    annotations.insert(
        format!("{}/i-dont-exist.foo", project::COPY_ANNOTATION_BASE),
        project::COPY_ANNOTATION_COPY_VALUE.to_string(),
    );
    meta_data.annotations = Some(annotations);

    let result = operator.admission_hook(project).await;

    match result {
        AdmissionResult::Deny(status) => {
            assert_eq!(status.code, Some(409));
            assert_eq!(
                status.message,
                Some("annotation 'project.selfservice.innoq.io/i-dont-exist.foo: copy' not possible: secret with name 'i-dont-exist' does not exist".to_string())
            );
            assert_eq!(status.status, Some("Failure".to_string()));
        }
        _ => panic!(
            "admission hook did not fail even though the project's annotations referenced a non existant secret"
        ),
    }

    Ok(())
}

#[tokio::test]
#[serial]
#[ignore = "not yet implemented"]
async fn it_should_if_secret_does_not_contain_addressed_data_item() -> anyhow::Result<()> {
    Ok(())
}
