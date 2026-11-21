use arena_store::catalog;
use arena_store::config::Config;
use arena_store::httpapi::{self, AppState};
use arena_store::orchestrator::LocalOrchestrator;
use arena_store::store::Memory;
use arena_store::workflow::Handler;
use axum::body::{to_bytes, Body, Bytes};
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

struct Server {
    app: axum::Router,
}

impl Server {
    async fn new() -> Self {
        let store = Arc::new(Memory::new());
        catalog::seed(store.as_ref()).await.unwrap();
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
        let handler = Handler::new(store.clone(), cfg.clone());
        let orch = Arc::new(LocalOrchestrator::new(
            handler,
            store.clone(),
            cfg.cart_ttl,
            cfg.pix_ttl,
        ));
        Self {
            app: httpapi::router(AppState { store, orch, cfg }),
        }
    }

    async fn call(&self, req: Request<Body>) -> (StatusCode, Bytes) {
        let response = self.app.clone().oneshot(req).await.unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, body)
    }
}

fn json_req(method: &str, uri: &str, token: Option<&str>, body: Option<Value>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header("Authorization", format!("Bearer {token}"));
    }
    if body.is_some() {
        builder = builder.header("Content-Type", "application/json");
    }
    builder
        .body(Body::from(body.map(|v| v.to_string()).unwrap_or_default()))
        .unwrap()
}

#[tokio::test]
async fn checkout_pix_and_confirm() {
    let server = Server::new().await;
    let (status, body) = server
        .call(json_req("POST", "/api/auth/guest", None, Some(json!({}))))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body:?}");
    let guest: Value = serde_json::from_slice(&body).unwrap();
    let token = guest["token"].as_str().unwrap();

    let (status, body) = server
        .call(json_req(
            "POST",
            "/api/cart/items",
            Some(token),
            Some(json!({"productId": "bola-futebol-pro", "quantity": 1})),
        ))
        .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");

    let mut order = Value::Null;
    for _ in 0..50 {
        let (status, body) = server
            .call(json_req(
                "POST",
                "/api/checkout",
                Some(token),
                Some(json!({"method": "pix"})),
            ))
            .await;
        if status == StatusCode::ACCEPTED {
            order = serde_json::from_slice(&body).unwrap();
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        order["id"].as_str().is_some(),
        true,
        "checkout did not start: {order}"
    );
    let order_id = order["id"].as_str().unwrap().to_string();

    let mut paid = Value::Null;
    for _ in 0..50 {
        let (status, body) = server
            .call(json_req(
                "GET",
                &format!("/api/orders/{order_id}"),
                Some(token),
                None,
            ))
            .await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        let current: Value = serde_json::from_slice(&body).unwrap();
        if current["status"] == "awaiting_pix" {
            let (status, body) = server
                .call(json_req(
                    "POST",
                    &format!("/api/orders/{order_id}/pix/confirm"),
                    Some(token),
                    None,
                ))
                .await;
            assert!(status.is_success(), "{body:?}");
        }
        if current["status"] == "paid" {
            paid = current;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(paid["status"], "paid", "{paid}");

    let (status, body) = server
        .call(json_req(
            "GET",
            "/api/products/bola-futebol-pro",
            None,
            None,
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    let product: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(product["available"], 39);
    assert_eq!(product["sold"], 1);
}
