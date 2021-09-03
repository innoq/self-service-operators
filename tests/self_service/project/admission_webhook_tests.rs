use crate::common;
use core::default::Default;
use core::option::Option::Some;
use core::result::Result::Ok;
use krator::admission::AdmissionResult;
use krator::Operator;
use kube::Resource;

use noqnoqnoq::self_service::project::project::{
    COPY_ANNOTATION_BASE, COPY_ANNOTATION_COPY_VALUE, DEFAULT_MANIFESTS_SECRET,
};
use noqnoqnoq::self_service::project::Project;
use serial_test::serial;
use std::collections::BTreeMap;

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
        format!("{}/i-dont-exist.foo", COPY_ANNOTATION_BASE),
        COPY_ANNOTATION_COPY_VALUE.to_string(),
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
async fn it_should_fail_if_secret_does_not_contain_addressed_data_item() -> anyhow::Result<()> {
    let (_, operator) = common::before_each().await?;

    let name = common::random_name("missing-secret-item");
    let mut project = Project::new(&name, Default::default());

    let mut meta_data = project.meta_mut();

    let mut annotations = BTreeMap::new();
    annotations.insert(
        format!("{}/{}.foo", COPY_ANNOTATION_BASE, DEFAULT_MANIFESTS_SECRET),
        COPY_ANNOTATION_COPY_VALUE.to_string(),
    );
    meta_data.annotations = Some(annotations);

    let result = operator.admission_hook(project).await;

    match result {
        AdmissionResult::Deny(status) => {
            assert_eq!(status.code, Some(409));
            assert_eq!(
                status.message,
                Some(format!("annotation 'project.selfservice.innoq.io/{}.foo: copy' not possible: secret '{}' does not contain a data item named 'foo'", DEFAULT_MANIFESTS_SECRET, DEFAULT_MANIFESTS_SECRET))
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
async fn it_should_fail_if_template_vals_are_missing() -> anyhow::Result<()> {
    let (client, operator) = common::before_each().await?;

    common::apply_manifest_secret(
        &client,
        DEFAULT_MANIFESTS_SECRET,
        vec![include_str!("../../fixtures/templated-pod.yaml")],
    )
    .await?;

    let name = common::random_name("missing-template-val");
    let project = Project::new(&name, Default::default());

    let result = operator.admission_hook(project).await;

    match result {
        AdmissionResult::Deny(status) => {
            assert_eq!(status.code, Some(409));
            assert_eq!(
                status.message,
                Some("Error rendering \"default-project-manifests/resource0\" line 5, col 9: Variable \"name\" not found in strict mode. (did you provide all necessary manifestValues in the project spec?)".to_string())
            );
            assert_eq!(status.status, Some("Failure".to_string()));
        }
        _ => panic!("admission hook did not fail even though a manifest value was missing"),
    }
    Ok(())
}

#[tokio::test]
#[serial]
async fn it_should_fail_if_manifest_values_is_not_a_yaml_string() -> anyhow::Result<()> {
    let (client, operator) = common::before_each().await?;

    common::apply_manifest_secret(
        &client,
        DEFAULT_MANIFESTS_SECRET,
        vec![include_str!("../../fixtures/pod.yaml")],
    )
    .await?;

    let name = common::random_name("missing-template-val");
    let mut project = Project::new(&name, Default::default());
    project.spec.manifest_values = Some("foo: -".into());

    let result = operator.admission_hook(project).await;

    match result {
        AdmissionResult::Deny(status) => {
            assert_eq!(status.code, Some(409));
            assert_eq!(status.message, Some("Invalid project spec: error parsing manifestValues which must be a string that represents a yaml mapping, got 'foo: -':\nblock sequence entries are not allowed in this context at line 1 column 6".to_string()));
            assert_eq!(status.status, Some("Failure".to_string()));
        }
        _ => panic!("admission hook did not fail even though manifestValues was invalid"),
    }
    Ok(())
}

#[tokio::test]
#[serial]
async fn it_should_fail_if_manifest_values_is_just_a_simple_string() -> anyhow::Result<()> {
    let (client, operator) = common::before_each().await?;

    common::apply_manifest_secret(
        &client,
        DEFAULT_MANIFESTS_SECRET,
        vec![include_str!("../../fixtures/pod.yaml")],
    )
    .await?;

    let name = common::random_name("missing-template-val");
    let mut project = Project::new(&name, Default::default());
    project.spec.manifest_values = Some("baaam".into());

    let result = operator.admission_hook(project).await;

    match result {
        AdmissionResult::Deny(status) => {
            assert_eq!(status.code, Some(409));
            assert_eq!(status.message, Some("Invalid project spec: property manifestValues must be a string that represents a yaml mapping, got a string with value 'baaam'".into()));
            assert_eq!(status.status, Some("Failure".to_string()));
        }
        _ => panic!("admission hook did not fail even though manifestValues was invalid"),
    }
    Ok(())
}

#[tokio::test]
#[serial]
async fn it_should_fail_if_manifest_values_is_an_array() -> anyhow::Result<()> {
    let (client, operator) = common::before_each().await?;

    common::apply_manifest_secret(
        &client,
        DEFAULT_MANIFESTS_SECRET,
        vec![include_str!("../../fixtures/pod.yaml")],
    )
    .await?;

    let name = common::random_name("missing-template-val");
    let mut project = Project::new(&name, Default::default());
    project.spec.manifest_values = Some("[1,2]".into());

    let result = operator.admission_hook(project).await;

    match result {
        AdmissionResult::Deny(status) => {
            assert_eq!(status.code, Some(409));
            assert_eq!(status.message, Some("Invalid project spec: property manifestValues must be a string that represents a yaml mapping, got an array with value '[1,2]'".into()));
            assert_eq!(status.status, Some("Failure".to_string()));
        }
        _ => panic!("admission hook did not fail even though manifestValues was invalid"),
    }
    Ok(())
}
