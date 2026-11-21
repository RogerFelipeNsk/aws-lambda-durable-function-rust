use super::{OrchError, Orchestrator};
use crate::domain::{CheckoutSignal, OrchestratorEvent, PixSignal};
use crate::store::Store;
use crate::workflow::Handler;
use async_trait::async_trait;
use aws_durable_execution_sdk::test_util::{
    CallbackFailure, CallbackReply, LocalRunner, WaitingStatus,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Mutex};

enum Cmd {
    Checkout {
        signal: CheckoutSignal,
        reply: oneshot::Sender<Result<(), String>>,
    },
    PixPaid {
        reply: oneshot::Sender<Result<(), String>>,
    },
    Fail {
        error_type: String,
        message: String,
        reply: oneshot::Sender<Result<(), String>>,
    },
}

pub struct LocalOrchestrator {
    handler: Handler,
    store: Arc<dyn Store>,
    cart_ttl: Duration,
    pix_ttl: Duration,
    sessions: Arc<Mutex<HashMap<String, mpsc::Sender<Cmd>>>>,
}

impl LocalOrchestrator {
    pub fn new(
        handler: Handler,
        store: Arc<dyn Store>,
        cart_ttl: Duration,
        pix_ttl: Duration,
    ) -> Self {
        Self {
            handler,
            store,
            cart_ttl,
            pix_ttl,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn send(
        &self,
        callback_id: &str,
        cmd: Cmd,
        reply: oneshot::Receiver<Result<(), String>>,
    ) -> Result<(), OrchError> {
        let tx = {
            let sessions = self.sessions.lock().await;
            sessions.get(callback_id).cloned()
        };
        let Some(tx) = tx else {
            return Err(OrchError::Message("callback not found".into()));
        };
        tx.send(cmd)
            .await
            .map_err(|_| OrchError::Message("sessão durable encerrada".into()))?;
        reply
            .await
            .map_err(|_| OrchError::Message("sessão durable encerrada".into()))?
            .map_err(OrchError::Message)
    }
}

#[async_trait]
impl Orchestrator for LocalOrchestrator {
    async fn start_cart_session(&self, cart_id: &str) -> Result<String, OrchError> {
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<Cmd>(8);
        let (ready_tx, ready_rx) = oneshot::channel::<Result<String, String>>();
        let handler = self.handler.clone();
        let store = self.store.clone();
        let cart_ttl = self.cart_ttl;
        let pix_ttl = self.pix_ttl;
        let sessions = self.sessions.clone();
        let cart_id_owned = cart_id.to_string();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("local durable runtime");
            rt.block_on(async move {
                let event = OrchestratorEvent {
                    cart_id: cart_id_owned.clone(),
                };
                let mut run = LocalRunner::new().run(
                    move |event, ctx| {
                        let handler = handler.clone();
                        async move { handler.run(event, ctx).await }
                    },
                    event,
                );

                let checkout = run.operation("await-checkout");
                if let Err(err) = checkout.wait_for_data(WaitingStatus::Submitted).await {
                    let _ = ready_tx.send(Err(err.to_string()));
                    return;
                }

                let checkout_id = match store.get_cart(&cart_id_owned).await {
                    Ok(cart) => cart.checkout_callback_id,
                    Err(err) => {
                        let _ = ready_tx.send(Err(err.to_string()));
                        return;
                    }
                };
                {
                    let mut map = sessions.lock().await;
                    map.insert(checkout_id.clone(), cmd_tx.clone());
                }
                let _ = ready_tx.send(Ok(checkout_id));

                let checkout_cmd = tokio::select! {
                    _ = tokio::time::sleep(cart_ttl) => None,
                    cmd = cmd_rx.recv() => cmd,
                };
                match checkout_cmd {
                    None => {
                        let _ = checkout.send_callback(CallbackReply::timeout()).await;
                    }
                    Some(Cmd::Checkout { signal, reply }) => {
                        let sent = match CallbackReply::success(&signal) {
                            Ok(body) => checkout
                                .send_callback(body)
                                .await
                                .map_err(|e| e.to_string()),
                            Err(err) => Err(err.to_string()),
                        };
                        let _ = reply.send(sent);
                    }
                    Some(Cmd::Fail {
                        error_type,
                        message,
                        reply,
                    }) => {
                        let fail = checkout
                            .send_callback(CallbackReply::failure(CallbackFailure::new(
                                error_type, message,
                            )))
                            .await;
                        let _ = reply.send(fail.map_err(|e| e.to_string()).map(|_| ()));
                    }
                    Some(Cmd::PixPaid { reply }) => {
                        let _ = reply.send(Err("checkout ainda não avançou".into()));
                    }
                }

                let pix = run.operation("await-pix");
                tokio::select! {
                    _ = &mut run => return,
                    wait = pix.wait_for_data(WaitingStatus::Submitted) => {
                        if wait.is_err() {
                            let _ = run.await;
                            return;
                        }
                    }
                }

                let pix_id = match store.get_cart(&cart_id_owned).await {
                    Ok(cart) => cart.pix_callback_id,
                    Err(_) => {
                        let _ = run.await;
                        return;
                    }
                };
                if !pix_id.is_empty() {
                    sessions.lock().await.insert(pix_id, cmd_tx.clone());
                }

                let pix_cmd = tokio::select! {
                    _ = tokio::time::sleep(pix_ttl) => None,
                    cmd = cmd_rx.recv() => cmd,
                };
                match pix_cmd {
                    None => {
                        let _ = pix.send_callback(CallbackReply::timeout()).await;
                    }
                    Some(Cmd::PixPaid { reply }) => {
                        let sent = match CallbackReply::success(&PixSignal { paid: true }) {
                            Ok(reply_body) => pix
                                .send_callback(reply_body)
                                .await
                                .map_err(|e| e.to_string()),
                            Err(err) => Err(err.to_string()),
                        };
                        let _ = reply.send(sent);
                    }
                    Some(Cmd::Fail {
                        error_type,
                        message,
                        reply,
                    }) => {
                        let fail = pix
                            .send_callback(CallbackReply::failure(CallbackFailure::new(
                                error_type, message,
                            )))
                            .await;
                        let _ = reply.send(fail.map_err(|e| e.to_string()).map(|_| ()));
                    }
                    Some(Cmd::Checkout { reply, .. }) => {
                        let _ = reply.send(Err("checkout já enviado".into()));
                    }
                }
                let _ = run.await;
            });
        });

        match ready_rx.await {
            Ok(Ok(_)) => Ok(format!("local:{cart_id}")),
            Ok(Err(err)) => Err(OrchError::Message(err)),
            Err(_) => Err(OrchError::Message(
                "falha ao iniciar sessão durable local".into(),
            )),
        }
    }

    async fn signal_checkout(
        &self,
        callback_id: &str,
        signal: CheckoutSignal,
    ) -> Result<(), OrchError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(
            callback_id,
            Cmd::Checkout {
                signal,
                reply: reply_tx,
            },
            reply_rx,
        )
        .await
    }

    async fn signal_pix_paid(&self, callback_id: &str) -> Result<(), OrchError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(callback_id, Cmd::PixPaid { reply: reply_tx }, reply_rx)
            .await
    }

    async fn signal_failure(
        &self,
        callback_id: &str,
        error_type: &str,
        message: &str,
    ) -> Result<(), OrchError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(
            callback_id,
            Cmd::Fail {
                error_type: error_type.into(),
                message: message.into(),
                reply: reply_tx,
            },
            reply_rx,
        )
        .await
    }
}
