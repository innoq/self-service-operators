use crate::{get_client, reinstall_self_service_crd};
use self_service_operators::postgres::operator::PostgresOperator;
use self_service_operators::postgres::Postgres;

mod states;

pub async fn before_each() -> anyhow::Result<(kube::Client, PostgresOperator)> {
    let (_config, client) = get_client().await?;

    // todo: do we really have to install the webhook resources for testing?
    // there is probably a better way FnOnce?
    let _ = reinstall_self_service_crd(
        &client,
        &Postgres::crd(),
        Postgres::admission_webhook_resources("default"),
    )
    .await?;

    unimplemented!();
    // let operator = self_service_operators::postgres::postgres::PostgresOperator::new(
    //     client.clone(),
    //     "default",
    // )
    // .await
    // .unwrap();
    // let mut runtime = OperatorRuntime::new(&config, operator.clone(), None);
    //
    // tokio::spawn(async move { runtime.start().await });
    //
    // Ok((client, operator))
}
