use crate::{get_client, reinstall_self_service_crd};
use anyhow::Context;
use krator::OperatorRuntime;
use self_service_operators::postgres::operator::PostgresOperator;
use self_service_operators::postgres::Postgres;
use tokio_postgres::NoTls;

mod states;

const TEST_DATABASE_CONNECTION_STRING: &str = "postgresql://postgres:geheim@localhost/postgres";

pub async fn before_each(
) -> anyhow::Result<(kube::Client, tokio_postgres::Client, PostgresOperator)> {
    let (config, kube_client) = get_client().await?;

    // todo: do we really have to install the webhook resources for testing?
    // there is probably a better way FnOnce?
    let _ = reinstall_self_service_crd(
        &kube_client,
        &Postgres::crd(),
        Postgres::admission_webhook_resources("default"),
    )
    .await?;

    let (postgres_client, _) = tokio_postgres::connect(TEST_DATABASE_CONNECTION_STRING, NoTls)
        .await
        .context(format!(
            "Couldn't connect to test database using connection string: {}",
            TEST_DATABASE_CONNECTION_STRING
        ))?;
    let operator = PostgresOperator::new(kube_client.clone(), postgres_client, "default")
        .await
        .unwrap();

    let mut runtime = OperatorRuntime::new(&config, operator.clone(), None);
    tokio::spawn(async move { runtime.start().await });

    // we don't understand lifetimes and Arcs so we simply create a new connection for the tests
    let (postgres_client, _) = tokio_postgres::connect(TEST_DATABASE_CONNECTION_STRING, NoTls)
        .await
        .context(format!(
            "Couldn't connect to test database using connection string: {}",
            TEST_DATABASE_CONNECTION_STRING
        ))?;
    Ok((kube_client, postgres_client, operator))
}
