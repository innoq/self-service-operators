mod common;
use k8s_openapi::api::core::v1::Secret;
use kube::api::{ObjectMeta, PostParams};
use noqnoqnoq::{
    helper,
    project::{self},
};
use serial_test::serial;
use std::collections::BTreeMap;

#[tokio::test]
#[serial]
async fn it_fails_with_non_existant_default_manifests_secret() -> anyhow::Result<()> {
    let (_, client) = common::get_client().await?;

    match project::ProjectOperator::new(
        client.clone(),
        project::OWNER_ROLE_BINDING_NAME,
        "default",
        "non-existant-secret",
    )
    .await
    {
        Ok(_) => panic!(
            "project operator should fail if the given default manifests secret does not exist"
        ),
        Err(e) => assert_eq!(
            e.to_string(),
            "no Secret with name 'non-existant-secret' in namespace 'default' found (this secret should hold default manifests that get applied in each new namespace) -- aborting",
            "error message should be correct"
        ),
    };
    Ok(())
}

#[tokio::test]
#[serial]
async fn it_should_only_copy_from_annotated_secrets() -> anyhow::Result<()> {
    let (client, _) = common::before_each().await?;

    let name = common::random_name("secret-annotations");
    let _ = common::install_project(&client, &name).await?;

    let api = kube::Api::<Secret>::namespaced(client.clone(), &name);

    let _standard_secret = api
        .create(
            &PostParams::default(),
            &Secret {
                data: Some(BTreeMap::new()),
                metadata: ObjectMeta {
                    name: Some("standard-secret".to_string()),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .await?;

    let mut annotations = BTreeMap::new();
    annotations.insert(
        project::SECRET_ANNOTATION_KEY.to_string(),
        project::SECRET_ANNOTATION_VALUE.to_string(),
    );
    let _annotated_secret = api
        .create(
            &PostParams::default(),
            &Secret {
                data: Some(BTreeMap::new()),
                metadata: ObjectMeta {
                    annotations: Some(annotations),
                    name: Some("annotated-secret".to_string()),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .await?;

    let resources = helper::get_manifests_secret(&client, "i-do-not-exist", &name).await;
    assert!(resources.is_err());
    assert_eq!(resources.unwrap_err().to_string(), "ApiError: secrets \"i-do-not-exist\" not found: NotFound (ErrorResponse { status: \"Failure\", message: \"secrets \\\"i-do-not-exist\\\" not found\", reason: \"NotFound\", code: 404 })");

    let resources = helper::get_manifests_secret(&client, "standard-secret", &name).await;
    assert!(resources.is_err());
    assert_eq!(
        resources.unwrap_err().to_string(),
        format!(
            "Error accessing secret 'standard-secret': only secrets with the annotation '{}: {}' can be accessed by the project operator",
            project::SECRET_ANNOTATION_KEY,
            project::SECRET_ANNOTATION_VALUE
        )
    );

    let resources = helper::get_manifests_secret(&client, "annotated-secret", &name).await;
    assert!(resources.is_ok());

    Ok(())
}


#[tokio::test]
#[serial]
#[ignore = "not yet implemented"]
async fn it_should_correctly_copy_default_manifests() -> anyhow::Result<()> {
    Ok(())
}

#[tokio::test]
#[serial]
#[ignore = "not yet implemented"]
async fn it_should_correctly_copy_annotated_manifests() -> anyhow::Result<()> {
    Ok(())
}
