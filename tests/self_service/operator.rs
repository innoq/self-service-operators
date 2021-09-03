use std::time::Duration;

use serial_test::serial;

use noqnoqnoq::self_service::operator;
use noqnoqnoq::self_service::project;

use crate::common;

#[tokio::test]
#[serial]
async fn it_fails_with_non_existant_default_manifests_secret() -> anyhow::Result<()> {
    let (_, client) = common::get_client().await?;

    match operator::ProjectOperator::new(
		client.clone(),
		project::OWNER_ROLE_BINDING_NAME,
		"default",
		"non-existant-secret",
        Duration::from_secs(0)
	)
	.await
	{
		Ok(_) => panic!(
			"project operator should fail if the given default manifests secret does not exist"
		),
		Err(e) => assert_eq!(
			e.to_string(),
			"no Secret with name 'non-existant-secret' in namespace 'default' found (this secret should hold default manifests that get applied in each new namespace): ApiError: secrets \"non-existant-secret\" not found: NotFound (ErrorResponse { status: \"Failure\", message: \"secrets \\\"non-existant-secret\\\" not found\", reason: \"NotFound\", code: 404 }) -- aborting",
            "error message should be correct"
		),
	};
    Ok(())
}

#[tokio::test]
#[serial]
async fn it_fails_with_non_existant_owner_default_role_binding() -> anyhow::Result<()> {
    let (_, client) = common::get_client().await?;

    assert!(
        common::apply_manifest_secret(
            &client,
            project::DEFAULT_MANIFESTS_SECRET,
            vec![include_str!("../fixtures/pod.yaml")]
        )
        .await
        .is_ok(),
        "installing default manifest secret should work"
    );

    match operator::ProjectOperator::new(
        client.clone(),
        "non-existant-cluster-role-name",
        "default",
        project::DEFAULT_MANIFESTS_SECRET,
        Duration::from_secs(0),
    )
    .await
    {
        Ok(_) => panic!(
            "project operator should fail if the given default owner cluster role does not exist"
        ),
        Err(e) => assert_eq!(
            e.to_string(),
            "no ClusterRole with name 'non-existant-cluster-role-name' found -- aborting",
            "error message should be correct"
        ),
    };
    Ok(())
}
