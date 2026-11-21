use arena_store::catalog;
use arena_store::config::Config;
use arena_store::httpapi::{self, AppState};
use arena_store::orchestrator::{AwsOrchestrator, LocalOrchestrator, Orchestrator};
use arena_store::store::{Dynamo, Memory, Store};
use arena_store::workflow::Handler;
use std::sync::Arc;
use tokio::net::TcpListener;

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
    let (store, orch) = wire(&cfg).await?;
    catalog::seed(store.as_ref()).await?;
    let app = httpapi::router(AppState {
        store,
        orch,
        cfg: cfg.clone(),
    });

    if std::env::var("AWS_LAMBDA_FUNCTION_NAME").is_ok() {
        lambda_http::run(app).await?;
        return Ok(());
    }

    let listener = TcpListener::bind(&cfg.listen_addr).await?;
    tracing::info!(addr = %cfg.listen_addr, mode = %mode(&cfg), "api listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn wire(
    cfg: &Config,
) -> Result<(Arc<dyn Store>, Arc<dyn Orchestrator>), Box<dyn std::error::Error + Send + Sync>> {
    if cfg.table_name.is_empty() {
        let store: Arc<dyn Store> = Arc::new(Memory::new());
        let handler = Handler::new(store.clone(), cfg.clone());
        let orch: Arc<dyn Orchestrator> = Arc::new(LocalOrchestrator::new(
            handler,
            store.clone(),
            cfg.cart_ttl,
            cfg.pix_ttl,
        ));
        return Ok((store, orch));
    }

    let aws_cfg = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let store: Arc<dyn Store> = Arc::new(Dynamo::new(
        aws_sdk_dynamodb::Client::new(&aws_cfg),
        cfg.table_name.clone(),
    ));
    if cfg.durable_function_name.is_empty() {
        let handler = Handler::new(store.clone(), cfg.clone());
        let orch: Arc<dyn Orchestrator> = Arc::new(LocalOrchestrator::new(
            handler,
            store.clone(),
            cfg.cart_ttl,
            cfg.pix_ttl,
        ));
        return Ok((store, orch));
    }
    let orch: Arc<dyn Orchestrator> = Arc::new(AwsOrchestrator::new(
        aws_sdk_lambda::Client::new(&aws_cfg),
        cfg.durable_function_name.clone(),
    ));
    Ok((store, orch))
}

fn mode(cfg: &Config) -> &'static str {
    if cfg.is_local() {
        "local-durable"
    } else {
        "aws-durable"
    }
}
