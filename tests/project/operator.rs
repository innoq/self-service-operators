use std::time::Duration;

use serial_test::serial;

use self_service_operators::project::operator;

use crate::{project};

#[tokio::test]
#[serial]
async fn it_fails_with_non_existant_default_manifests_secret() -> anyhow::Result<()> {
    let (_, client) = project::get_client().await?;

    match operator::ProjectOperator::new(
		client.clone(),
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
