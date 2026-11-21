use crate::config::Config;
use crate::domain::{
    CheckoutSignal, OrchestratorEvent, OrchestratorResult, PixCharge, PixSignal, CART_CHECKING_OUT,
    CART_EXPIRED, CART_OPEN, CART_PAID, CART_PAYMENT_FAILED, ORDER_AWAITING_PIX, ORDER_CANCELLED,
    ORDER_DECLINED, ORDER_PAID, ORDER_PENDING_PAYMENT, ORDER_PIX_EXPIRED, ORDER_PROCESSING_CARD,
    PAY_CARD, PAY_PIX,
};
use crate::payment::{self, PaymentError};
use crate::pix;
use crate::store::{self, Store, StoreError};
use aws_durable_execution_sdk::builders::RetryStrategyConfig;
use aws_durable_execution_sdk::{
    BoxError, CallbackErrorKind, DurableContext, OperationError, OperationErrorKind,
};
use chrono::Utc;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct Handler {
    store: Arc<dyn Store>,
    cfg: Config,
}

impl Handler {
    pub fn new(store: Arc<dyn Store>, cfg: Config) -> Self {
        Self { store, cfg }
    }

    pub async fn run(
        &self,
        event: OrchestratorEvent,
        ctx: DurableContext,
    ) -> Result<OrchestratorResult, BoxError> {
        if event.cart_id.is_empty() {
            return Err("cartId is required".into());
        }

        let store = self.store.clone();
        let cfg = self.cfg.clone();
        let cart_id = event.cart_id.clone();
        let signal = match ctx
            .wait_for_callback::<CheckoutSignal>(move |_sc, callback_id| {
                let store = store.clone();
                let cfg = cfg.clone();
                let cart_id = cart_id.clone();
                async move { on_checkout_ready(store.as_ref(), &cfg, &cart_id, &callback_id).await }
            })
            .name("await-checkout")
            .timeout(self.cfg.cart_ttl)
            .submitter_retry_config(RetryStrategyConfig::no_retry())
            .await
        {
            Ok(signal) => signal,
            Err(err) if is_callback_gone(&err) => {
                return self
                    .expire_cart(
                        &ctx,
                        &event,
                        "carrinho expirado — unidades reservadas voltaram ao estoque",
                    )
                    .await;
            }
            Err(err) => return Err(err.into()),
        };

        let store = self.store.clone();
        let cart_id = event.cart_id.clone();
        let signal_clone = signal.clone();
        ctx.step(move |_sc| {
            let store = store.clone();
            let cart_id = cart_id.clone();
            let signal = signal_clone.clone();
            async move { mark_checking_out(store.as_ref(), &cart_id, &signal).await }
        })
        .name("mark-checkout")
        .retry_strategy_config(RetryStrategyConfig::no_retry())
        .await?;

        match signal.method.as_str() {
            PAY_PIX => self.run_pix(&ctx, &event, &signal).await,
            PAY_CARD => self.run_card(&ctx, &event, &signal).await,
            _ => {
                self.fail_payment(
                    &ctx,
                    &event,
                    &signal.order_id,
                    "método de pagamento inválido",
                )
                .await
            }
        }
    }

    async fn run_pix(
        &self,
        ctx: &DurableContext,
        event: &OrchestratorEvent,
        signal: &CheckoutSignal,
    ) -> Result<OrchestratorResult, BoxError> {
        let store = self.store.clone();
        let cfg = self.cfg.clone();
        let cart_id = event.cart_id.clone();
        let order_id = signal.order_id.clone();
        match ctx
            .wait_for_callback::<PixSignal>(move |_sc, callback_id| {
                let store = store.clone();
                let cfg = cfg.clone();
                let cart_id = cart_id.clone();
                let order_id = order_id.clone();
                async move { start_pix(store.as_ref(), &cfg, &cart_id, &order_id, &callback_id).await }
            })
            .name("await-pix")
            .timeout(self.cfg.pix_ttl)
            .submitter_retry_config(RetryStrategyConfig::no_retry())
            .await
        {
            Ok(_) => self.commit_paid(ctx, event, &signal.order_id, PAY_PIX, "").await,
            Err(err) if is_callback_gone(&err) => self.expire_pix(ctx, event, &signal.order_id).await,
            Err(err) => Err(err.into()),
        }
    }

    async fn run_card(
        &self,
        ctx: &DurableContext,
        event: &OrchestratorEvent,
        signal: &CheckoutSignal,
    ) -> Result<OrchestratorResult, BoxError> {
        let delay = if signal.simulate == "card_delay" {
            self.cfg.card_delay_long
        } else {
            self.cfg.card_delay
        };
        let simulate = signal.simulate.clone();
        let card = signal.card.clone();
        let last4 = match ctx
            .step(move |_sc| {
                let simulate = simulate.clone();
                let card = card.clone();
                async move { charge_card(delay, &simulate, card.as_ref()).await }
            })
            .name("charge-card")
            .retry_strategy_config(RetryStrategyConfig::no_retry())
            .await
        {
            Ok(last4) => last4,
            Err(err) => {
                return self
                    .fail_payment(ctx, event, &signal.order_id, format!("{err:#}"))
                    .await;
            }
        };
        self.commit_paid(ctx, event, &signal.order_id, PAY_CARD, &last4)
            .await
    }

    async fn commit_paid(
        &self,
        ctx: &DurableContext,
        event: &OrchestratorEvent,
        order_id: &str,
        method: &str,
        last4: &str,
    ) -> Result<OrchestratorResult, BoxError> {
        let store = self.store.clone();
        let cart_id = event.cart_id.clone();
        let order_id_owned = order_id.to_string();
        let method = method.to_string();
        let last4 = last4.to_string();
        ctx.step(move |_sc| {
            let store = store.clone();
            let cart_id = cart_id.clone();
            let order_id = order_id_owned.clone();
            let method = method.clone();
            let last4 = last4.clone();
            async move { commit_stock(store.as_ref(), &cart_id, &order_id, &method, &last4).await }
        })
        .name("commit-stock")
        .retry_strategy_config(RetryStrategyConfig::no_retry())
        .await?;
        Ok(OrchestratorResult {
            cart_id: event.cart_id.clone(),
            order_id: order_id.to_string(),
            status: ORDER_PAID.into(),
        })
    }

    async fn expire_cart(
        &self,
        ctx: &DurableContext,
        event: &OrchestratorEvent,
        message: &str,
    ) -> Result<OrchestratorResult, BoxError> {
        let store = self.store.clone();
        let cart_id = event.cart_id.clone();
        let message = message.to_string();
        ctx.step(move |_sc| {
            let store = store.clone();
            let cart_id = cart_id.clone();
            let message = message.clone();
            async move { expire_cart_stock(store.as_ref(), &cart_id, &message).await }
        })
        .name("expire-cart")
        .retry_strategy_config(RetryStrategyConfig::no_retry())
        .await?;
        Ok(OrchestratorResult {
            cart_id: event.cart_id.clone(),
            order_id: String::new(),
            status: CART_EXPIRED.into(),
        })
    }

    async fn expire_pix(
        &self,
        ctx: &DurableContext,
        event: &OrchestratorEvent,
        order_id: &str,
    ) -> Result<OrchestratorResult, BoxError> {
        let store = self.store.clone();
        let cart_id = event.cart_id.clone();
        let order_id_owned = order_id.to_string();
        ctx.step(move |_sc| {
            let store = store.clone();
            let cart_id = cart_id.clone();
            let order_id = order_id_owned.clone();
            async move { expire_pix_stock(store.as_ref(), &cart_id, &order_id).await }
        })
        .name("expire-pix")
        .retry_strategy_config(RetryStrategyConfig::no_retry())
        .await?;
        Ok(OrchestratorResult {
            cart_id: event.cart_id.clone(),
            order_id: order_id.to_string(),
            status: ORDER_PIX_EXPIRED.into(),
        })
    }

    async fn fail_payment(
        &self,
        ctx: &DurableContext,
        event: &OrchestratorEvent,
        order_id: &str,
        message: impl Into<String>,
    ) -> Result<OrchestratorResult, BoxError> {
        let store = self.store.clone();
        let cart_id = event.cart_id.clone();
        let order_id_owned = order_id.to_string();
        let message = message.into();
        ctx.step(move |_sc| {
            let store = store.clone();
            let cart_id = cart_id.clone();
            let order_id = order_id_owned.clone();
            let message = message.clone();
            async move { fail_payment_stock(store.as_ref(), &cart_id, &order_id, &message).await }
        })
        .name("fail-payment")
        .retry_strategy_config(RetryStrategyConfig::no_retry())
        .await?;
        Ok(OrchestratorResult {
            cart_id: event.cart_id.clone(),
            order_id: order_id.to_string(),
            status: ORDER_DECLINED.into(),
        })
    }
}

async fn on_checkout_ready(
    store: &dyn Store,
    cfg: &Config,
    cart_id: &str,
    callback_id: &str,
) -> Result<(), BoxError> {
    let mut cart = store.get_cart(cart_id).await?;
    cart.checkout_callback_id = callback_id.to_string();
    cart.status = CART_OPEN.into();
    cart.expires_at =
        Some(Utc::now() + chrono::Duration::from_std(cfg.cart_ttl).unwrap_or_default());
    store::append_cart_event(
        &mut cart,
        "session_open",
        format!("reserva ativa por {:?}", cfg.cart_ttl),
    );
    store.put_cart(cart).await?;
    Ok(())
}

async fn mark_checking_out(
    store: &dyn Store,
    cart_id: &str,
    signal: &CheckoutSignal,
) -> Result<String, BoxError> {
    let mut cart = store.get_cart(cart_id).await?;
    cart.status = CART_CHECKING_OUT.into();
    cart.order_id = signal.order_id.clone();
    store::append_cart_event(
        &mut cart,
        "checkout",
        format!("checkout iniciado via {}", signal.method),
    );
    store.put_cart(cart).await?;

    let mut order = store.get_order(&signal.order_id).await?;
    if signal.method == PAY_CARD {
        order.status = ORDER_PROCESSING_CARD.into();
        store::append_order_event(
            &mut order,
            "card_processing",
            "enviando cobrança à operadora fictícia",
        );
    } else {
        order.status = ORDER_PENDING_PAYMENT.into();
        store::append_order_event(&mut order, "pix_prepare", "gerando cobrança PIX");
    }
    store.put_order(order).await?;
    Ok(String::new())
}

async fn start_pix(
    store: &dyn Store,
    cfg: &Config,
    cart_id: &str,
    order_id: &str,
    callback_id: &str,
) -> Result<(), BoxError> {
    let mut order = store.get_order(order_id).await?;
    let expires = Utc::now() + chrono::Duration::from_std(cfg.pix_ttl).unwrap_or_default();
    order.pix = Some(PixCharge {
        tx_id: order_id.to_string(),
        copia_e_cola: pix::copia_e_cola(
            order_id,
            "Arena Store Demo",
            "SAO PAULO",
            "arena@demo.local",
            order.total_cents,
        ),
        expires_at: expires,
    });
    order.status = ORDER_AWAITING_PIX.into();
    store::append_order_event(
        &mut order,
        "pix_qr",
        format!("QR PIX gerado, expira em {:?}", cfg.pix_ttl),
    );
    store.put_order(order).await?;

    let mut cart = store.get_cart(cart_id).await?;
    cart.pix_callback_id = callback_id.to_string();
    store::append_cart_event(&mut cart, "pix_waiting", "aguardando pagamento PIX");
    store.put_cart(cart).await?;
    Ok(())
}

async fn charge_card(
    delay: Duration,
    simulate: &str,
    card: Option<&crate::domain::CardData>,
) -> Result<String, BoxError> {
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
    if simulate == "card_fail" {
        return Err(PaymentError::Declined.into());
    }
    let input = card
        .map(|c| payment::Input {
            number: c.number.clone(),
            exp: c.exp.clone(),
            cvc: c.cvc.clone(),
        })
        .unwrap_or_default();
    payment::validate(&input)?;
    Ok(payment::last4(&input.number))
}

async fn commit_stock(
    store: &dyn Store,
    cart_id: &str,
    order_id: &str,
    method: &str,
    last4: &str,
) -> Result<String, BoxError> {
    store.commit_cart(cart_id).await?;
    let mut cart = store.get_cart(cart_id).await?;
    cart.status = CART_PAID.into();
    cart.expires_at = None;
    store::append_cart_event(&mut cart, "paid", "pagamento confirmado, estoque baixado");
    store.put_cart(cart).await?;

    let mut order = store.get_order(order_id).await?;
    order.status = ORDER_PAID.into();
    order.card_last4 = last4.to_string();
    let msg = if method == PAY_PIX {
        "PIX confirmado"
    } else {
        "cartão aprovado"
    };
    store::append_order_event(&mut order, "paid", msg);
    store.put_order(order).await?;
    Ok(String::new())
}

async fn expire_cart_stock(
    store: &dyn Store,
    cart_id: &str,
    message: &str,
) -> Result<String, BoxError> {
    store.release_cart(cart_id).await?;
    let mut cart = store.get_cart(cart_id).await?;
    cart.status = CART_EXPIRED.into();
    store::append_cart_event(&mut cart, "expired", message);
    let order_id = cart.order_id.clone();
    store.put_cart(cart).await?;
    if order_id.is_empty() {
        return Ok(String::new());
    }
    match store.get_order(&order_id).await {
        Ok(mut order) => {
            order.status = ORDER_CANCELLED.into();
            store::append_order_event(&mut order, "cancelled", message);
            store.put_order(order).await?;
        }
        Err(StoreError::NotFound) => {}
        Err(err) => return Err(err.into()),
    }
    Ok(String::new())
}

async fn expire_pix_stock(
    store: &dyn Store,
    cart_id: &str,
    order_id: &str,
) -> Result<String, BoxError> {
    store.release_cart(cart_id).await?;
    let mut cart = store.get_cart(cart_id).await?;
    cart.status = CART_PAYMENT_FAILED.into();
    store::append_cart_event(&mut cart, "pix_expired", "PIX expirado — estoque devolvido");
    store.put_cart(cart).await?;
    let mut order = store.get_order(order_id).await?;
    order.status = ORDER_PIX_EXPIRED.into();
    store::append_order_event(
        &mut order,
        "pix_expired",
        "cobrança PIX expirou sem pagamento",
    );
    store.put_order(order).await?;
    Ok(String::new())
}

async fn fail_payment_stock(
    store: &dyn Store,
    cart_id: &str,
    order_id: &str,
    message: &str,
) -> Result<String, BoxError> {
    store.release_cart(cart_id).await?;
    let mut cart = store.get_cart(cart_id).await?;
    cart.status = CART_PAYMENT_FAILED.into();
    store::append_cart_event(&mut cart, "payment_failed", message);
    store.put_cart(cart).await?;
    if order_id.is_empty() {
        return Ok(String::new());
    }
    let mut order = store.get_order(order_id).await?;
    order.status = ORDER_DECLINED.into();
    store::append_order_event(&mut order, "declined", message);
    store.put_order(order).await?;
    Ok(String::new())
}

fn is_callback_gone(err: &OperationError) -> bool {
    matches!(
        err.kind(),
        OperationErrorKind::Callback(cb)
            if matches!(
                cb.kind(),
                CallbackErrorKind::TimedOut { .. }
                    | CallbackErrorKind::HeartbeatTimedOut { .. }
                    | CallbackErrorKind::ExternalFailure { .. }
            )
    )
}
