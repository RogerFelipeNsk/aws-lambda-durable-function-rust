use super::{OrchError, Orchestrator};
use crate::domain::{CheckoutSignal, OrchestratorEvent, PixSignal};
use async_trait::async_trait;
use aws_sdk_lambda::types::{ErrorObject, InvocationType};
use aws_sdk_lambda::Client;
use aws_smithy_types::Blob;

pub struct AwsOrchestrator {
    client: Client,
    function_name: String,
}

impl AwsOrchestrator {
    pub fn new(client: Client, function_name: impl Into<String>) -> Self {
        Self {
            client,
            function_name: function_name.into(),
        }
    }
}

#[async_trait]
impl Orchestrator for AwsOrchestrator {
    async fn start_cart_session(&self, cart_id: &str) -> Result<String, OrchError> {
        let payload = serde_json::to_vec(&OrchestratorEvent {
            cart_id: cart_id.to_string(),
        })
        .map_err(|e| OrchError::Message(e.to_string()))?;
        let out = self
            .client
            .invoke()
            .function_name(&self.function_name)
            .invocation_type(InvocationType::Event)
            .durable_execution_name(cart_id)
            .payload(Blob::new(payload))
            .send()
            .await
            .map_err(|e| OrchError::Message(e.to_string()))?;
        Ok(out
            .durable_execution_arn
            .unwrap_or_else(|| cart_id.to_string()))
    }

    async fn signal_checkout(
        &self,
        callback_id: &str,
        signal: CheckoutSignal,
    ) -> Result<(), OrchError> {
        let result = serde_json::to_vec(&signal).map_err(|e| OrchError::Message(e.to_string()))?;
        self.client
            .send_durable_execution_callback_success()
            .callback_id(callback_id)
            .result(Blob::new(result))
            .send()
            .await
            .map_err(|e| OrchError::Message(e.to_string()))?;
        Ok(())
    }

    async fn signal_pix_paid(&self, callback_id: &str) -> Result<(), OrchError> {
        let result = serde_json::to_vec(&PixSignal { paid: true })
            .map_err(|e| OrchError::Message(e.to_string()))?;
        self.client
            .send_durable_execution_callback_success()
            .callback_id(callback_id)
            .result(Blob::new(result))
            .send()
            .await
            .map_err(|e| OrchError::Message(e.to_string()))?;
        Ok(())
    }

    async fn signal_failure(
        &self,
        callback_id: &str,
        error_type: &str,
        message: &str,
    ) -> Result<(), OrchError> {
        self.client
            .send_durable_execution_callback_failure()
            .callback_id(callback_id)
            .error(
                ErrorObject::builder()
                    .error_type(error_type)
                    .error_message(message)
                    .build(),
            )
            .send()
            .await
            .map_err(|e| OrchError::Message(e.to_string()))?;
        Ok(())
    }
}
