use k8s_openapi::api::core::v1::Namespace;
use krator::OperatorRuntime;
use kube::api::PostParams;
use noqnoqnoq::project;
use noqnoqnoq::self_service::Sample;

mod common;

#[tokio::test]
async fn it_creates_namespace() -> anyhow::Result<()> {
    let (config, client) = common::k8sclient().await?;
    common::reinstall_self_service_crd(&client).await?;

    let mut runtime = OperatorRuntime::new(&config, project::ProjectOperator::new(), None);
    tokio::spawn(async move {
        runtime.start().await;
    });

    let name = common::random_name("project");

    let api: kube::Api<project::Project> = kube::Api::all(client.clone());
    let project = project::Project::new(name.as_str(), project::ProjectSpec::sample());

    let project_create_handle;
    {
        let client = client.clone();
        let name = name.clone();
        project_create_handle = tokio::spawn(async move {
            common::wait_for_state::<project::Project>(client, name, common::WaitForState::Created)
                .await
        });
    }
    let namespace_create_handle;
    {
        let client = client.clone();
        let name = name.clone();
        namespace_create_handle = tokio::spawn(async move {
            common::wait_for_state::<Namespace>(client, name, common::WaitForState::Created).await
        });
    }
    api.create(&PostParams::default(), &project).await?;
    assert_eq!(true, project_create_handle.await.is_ok());
    assert_eq!(true, namespace_create_handle.await.is_ok());

    Ok(())
}
