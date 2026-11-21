use arena_store::config::Config;
use arena_store::store::Dynamo;
use arena_store::workflow::Handler;
use aws_durable_execution_sdk as durable;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .json()
        .init();

    let cfg = Config::from_env();
    let aws_cfg = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let store = Arc::new(Dynamo::new(
        aws_sdk_dynamodb::Client::new(&aws_cfg),
        cfg.table_name.clone(),
    ));
    let handler = Handler::new(store, cfg);
    durable::run(move |event, ctx| {
        let handler = handler.clone();
        async move { handler.run(event, ctx).await }
    })
    .await?;
    Ok(())
}
