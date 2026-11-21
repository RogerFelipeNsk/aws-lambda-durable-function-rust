use crate::catalog;
use crate::config::Config;
use crate::domain::{
    CardData, Cart, CartItem, CheckoutSignal, OrchestratorEvent, Order, CART_EXPIRED,
    CART_STARTING, CURRENCY_BRL, ORDER_DECLINED, ORDER_PAID, ORDER_PENDING_PAYMENT,
    ORDER_PIX_EXPIRED, PAY_CARD, PAY_PIX,
};
use crate::payment;
use crate::store::{Memory, Store};
use crate::workflow::Handler;
use aws_durable_execution_sdk::test_util::{CallbackReply, LocalRunner};
use chrono::Utc;
use std::sync::Arc;
use std::time::Duration;

const PRODUCT_ID: &str = "bola-futebol-pro";

async fn setup() -> (Arc<Memory>, Handler, OrchestratorEvent) {
    let store = Arc::new(Memory::new());
    catalog::seed(store.as_ref()).await.unwrap();
    let now = Utc::now();
    let cart = Cart {
        id: "cart_test".into(),
        user_id: "usr_test".into(),
        status: CART_STARTING.into(),
        items: vec![CartItem {
            product_id: PRODUCT_ID.into(),
            name: "Bola".into(),
            emoji: "⚽".into(),
            unit_cents: 19990,
            quantity: 2,
        }],
        execution_arn: String::new(),
        checkout_callback_id: String::new(),
        pix_callback_id: String::new(),
        order_id: String::new(),
        expires_at: None,
        events: Vec::new(),
        created_at: now,
        updated_at: now,
    };
    store.put_cart(cart.clone()).await.unwrap();
    store.reserve(&cart.id, PRODUCT_ID, 2).await.unwrap();
    let cfg = Config {
        listen_addr: "127.0.0.1:0".into(),
        table_name: String::new(),
        durable_function_name: String::new(),
        allowed_origin: "*".into(),
        cart_ttl: Duration::from_secs(3600),
        pix_ttl: Duration::from_secs(3600),
        card_delay: Duration::ZERO,
        card_delay_long: Duration::ZERO,
        session_ttl: Duration::from_secs(3600),
    };
    let handler = Handler::new(store.clone(), cfg);
    (store, handler, OrchestratorEvent { cart_id: cart.id })
}

async fn place_order(store: &dyn Store, method: &str) -> Order {
    let now = Utc::now();
    let order = Order {
        id: "ord_test".into(),
        user_id: "usr_test".into(),
        cart_id: "cart_test".into(),
        status: ORDER_PENDING_PAYMENT.into(),
        method: method.into(),
        items: vec![CartItem {
            product_id: PRODUCT_ID.into(),
            name: "Bola".into(),
            emoji: "⚽".into(),
            unit_cents: 19990,
            quantity: 2,
        }],
        total_cents: 39980,
        currency: CURRENCY_BRL.into(),
        pix: None,
        card_last4: String::new(),
        simulate: String::new(),
        events: Vec::new(),
        created_at: now,
        updated_at: now,
    };
    store.put_order(order.clone()).await.unwrap();
    order
}

fn run_handler(
    handler: Handler,
) -> impl Fn(
    OrchestratorEvent,
    aws_durable_execution_sdk::DurableContext,
) -> std::pin::Pin<
    Box<
        dyn std::future::Future<
                Output = Result<
                    crate::domain::OrchestratorResult,
                    aws_durable_execution_sdk::BoxError,
                >,
            > + Send,
    >,
> {
    move |event, ctx| {
        let handler = handler.clone();
        Box::pin(async move { handler.run(event, ctx).await })
    }
}

#[tokio::test]
async fn pix_paid_commits_stock() {
    let (store, handler, event) = setup().await;
    let order = place_order(store.as_ref(), PAY_PIX).await;
    let result = LocalRunner::new()
        .callback_for(
            "await-checkout",
            CallbackReply::success(&CheckoutSignal {
                order_id: order.id.clone(),
                method: PAY_PIX.into(),
                card: None,
                simulate: String::new(),
            })
            .unwrap(),
        )
        .callback_for(
            "await-pix",
            CallbackReply::success(&crate::domain::PixSignal { paid: true }).unwrap(),
        )
        .run(run_handler(handler), event)
        .await
        .expect("runner");
    assert!(result.is_success(), "{result:?}");
    let p = store.get_product(PRODUCT_ID).await.unwrap();
    assert_eq!((p.available, p.reserved, p.sold), (38, 0, 2));
    let got = store.get_order(&order.id).await.unwrap();
    assert_eq!(got.status, ORDER_PAID);
}

#[tokio::test]
async fn pix_expired_restores_stock() {
    let (store, handler, event) = setup().await;
    let order = place_order(store.as_ref(), PAY_PIX).await;
    let result = LocalRunner::new()
        .callback_for(
            "await-checkout",
            CallbackReply::success(&CheckoutSignal {
                order_id: order.id.clone(),
                method: PAY_PIX.into(),
                card: None,
                simulate: String::new(),
            })
            .unwrap(),
        )
        .callback_for("await-pix", CallbackReply::timeout())
        .run(run_handler(handler), event)
        .await
        .expect("runner");
    assert!(result.is_success(), "{result:?}");
    let p = store.get_product(PRODUCT_ID).await.unwrap();
    assert_eq!((p.available, p.reserved, p.sold), (40, 0, 0));
    let got = store.get_order(&order.id).await.unwrap();
    assert_eq!(got.status, ORDER_PIX_EXPIRED);
}

#[tokio::test]
async fn cart_expired_restores_stock() {
    let (store, handler, event) = setup().await;
    let result = LocalRunner::new()
        .callback_for("await-checkout", CallbackReply::timeout())
        .run(run_handler(handler), event.clone())
        .await
        .expect("runner");
    assert!(result.is_success(), "{result:?}");
    let p = store.get_product(PRODUCT_ID).await.unwrap();
    assert_eq!((p.available, p.reserved), (40, 0));
    let cart = store.get_cart(&event.cart_id).await.unwrap();
    assert_eq!(cart.status, CART_EXPIRED);
}

#[tokio::test]
async fn card_paid() {
    let (store, handler, event) = setup().await;
    let order = place_order(store.as_ref(), PAY_CARD).await;
    let result = LocalRunner::new()
        .callback_for(
            "await-checkout",
            CallbackReply::success(&CheckoutSignal {
                order_id: order.id.clone(),
                method: PAY_CARD.into(),
                card: Some(CardData {
                    number: payment::TEST_NUMBER.into(),
                    exp: payment::TEST_EXP.into(),
                    cvc: payment::TEST_CVC.into(),
                }),
                simulate: String::new(),
            })
            .unwrap(),
        )
        .run(run_handler(handler), event)
        .await
        .expect("runner");
    assert!(result.is_success(), "{result:?}");
    let p = store.get_product(PRODUCT_ID).await.unwrap();
    assert_eq!((p.available, p.sold), (38, 2));
    let got = store.get_order(&order.id).await.unwrap();
    assert_eq!(got.status, ORDER_PAID);
    assert_eq!(got.card_last4, "1111");
}

#[tokio::test]
async fn card_declined_restores_stock() {
    let (store, handler, event) = setup().await;
    let order = place_order(store.as_ref(), PAY_CARD).await;
    let result = LocalRunner::new()
        .callback_for(
            "await-checkout",
            CallbackReply::success(&CheckoutSignal {
                order_id: order.id.clone(),
                method: PAY_CARD.into(),
                card: Some(CardData {
                    number: payment::TEST_NUMBER.into(),
                    exp: payment::TEST_EXP.into(),
                    cvc: payment::TEST_CVC.into(),
                }),
                simulate: "card_fail".into(),
            })
            .unwrap(),
        )
        .run(run_handler(handler), event)
        .await
        .expect("runner");
    assert!(result.is_success(), "{result:?}");
    let p = store.get_product(PRODUCT_ID).await.unwrap();
    assert_eq!((p.available, p.sold), (40, 0));
    let got = store.get_order(&order.id).await.unwrap();
    assert_eq!(got.status, ORDER_DECLINED);
}
