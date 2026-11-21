use crate::domain::CheckoutSignal;
use async_trait::async_trait;
use thiserror::Error;

mod aws;
mod local;

pub use aws::AwsOrchestrator;
pub use local::LocalOrchestrator;

#[derive(Debug, Error)]
pub enum OrchError {
    #[error("{0}")]
    Message(String),
}

#[async_trait]
pub trait Orchestrator: Send + Sync {
    async fn start_cart_session(&self, cart_id: &str) -> Result<String, OrchError>;
    async fn signal_checkout(
        &self,
        callback_id: &str,
        signal: CheckoutSignal,
    ) -> Result<(), OrchError>;
    async fn signal_pix_paid(&self, callback_id: &str) -> Result<(), OrchError>;
    async fn signal_failure(
        &self,
        callback_id: &str,
        error_type: &str,
        message: &str,
    ) -> Result<(), OrchError>;
}
