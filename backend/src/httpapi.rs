use crate::config::Config;
use crate::domain::{
    CardData, Cart, CartItem, CheckoutSignal, Order, Session, User, CART_OPEN, CART_STARTING,
    ORDER_AWAITING_PIX, ORDER_PENDING_PAYMENT, PAY_CARD, PAY_PIX,
};
use crate::guest;
use crate::id;
use crate::orchestrator::Orchestrator;
use crate::payment::{self, PaymentError};
use crate::store::{self, Store, StoreError};
use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Extension, Json, Router};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<dyn Store>,
    pub orch: Arc<dyn Orchestrator>,
    pub cfg: Config,
}

pub fn router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_headers(Any)
        .allow_methods(Any);
    let authed = Router::new()
        .route("/api/me", get(me))
        .route("/api/cart", get(get_cart))
        .route("/api/cart/items", post(add_item))
        .route(
            "/api/cart/items/{id}",
            patch(update_item).delete(remove_item),
        )
        .route("/api/checkout", post(checkout))
        .route("/api/orders", get(list_orders))
        .route("/api/orders/{id}", get(get_order))
        .route("/api/orders/{id}/pix/confirm", post(confirm_pix))
        .route("/api/orders/{id}/pix/expire", post(expire_pix))
        .route_layer(from_fn_with_state(state.clone(), require_user));
    Router::new()
        .route("/health", get(health))
        .route("/api/products", get(list_products))
        .route("/api/products/{id}", get(get_product))
        .route("/api/auth/guest", post(guest_login))
        .merge(authed)
        .layer(cors)
        .with_state(state)
}

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}

impl From<StoreError> for ApiError {
    fn from(err: StoreError) -> Self {
        match err {
            StoreError::NotFound => ApiError(StatusCode::NOT_FOUND, "não encontrado".into()),
            StoreError::InsufficientStock => ApiError(StatusCode::CONFLICT, err.to_string()),
            StoreError::CartClosed | StoreError::Conflict => {
                ApiError(StatusCode::CONFLICT, err.to_string())
            }
            StoreError::SessionExpired => ApiError(StatusCode::UNAUTHORIZED, err.to_string()),
            StoreError::Other(msg) => ApiError(StatusCode::INTERNAL_SERVER_ERROR, msg),
        }
    }
}

async fn health(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "service": "arena-store",
        "runtime": "rust",
        "mode": if state.cfg.is_local() { "local" } else { "aws" },
    }))
}

async fn list_products(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::domain::Product>>, ApiError> {
    let products = state.store.list_products().await?;
    Ok(Json(
        products
            .into_iter()
            .map(crate::domain::Product::public)
            .collect(),
    ))
}

async fn get_product(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<crate::domain::Product>, ApiError> {
    Ok(Json(state.store.get_product(&id).await?.public()))
}

async fn guest_login(State(state): State<AppState>) -> Result<(StatusCode, Json<Value>), ApiError> {
    let user = guest::new_user();
    state.store.create_user(user.clone()).await?;
    let session = Session {
        id: id::session(),
        user_id: user.id.clone(),
        created_at: Utc::now(),
        expires_at: Utc::now()
            + chrono::Duration::from_std(state.cfg.session_ttl)
                .unwrap_or(chrono::Duration::days(7)),
    };
    state.store.create_session(session.clone()).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "token": session.id,
            "user": user,
            "expiresAt": session.expires_at,
        })),
    ))
}

async fn me(Extension(user): Extension<User>) -> Json<User> {
    Json(user)
}

async fn get_cart(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
) -> Result<Json<Value>, ApiError> {
    match state.store.get_open_cart_by_user(&user.id).await {
        Ok(cart) => Ok(Json(cart_view(cart))),
        Err(StoreError::NotFound) => Ok(Json(json!({
            "items": [],
            "status": "empty",
            "totalCents": 0,
        }))),
        Err(err) => Err(err.into()),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemBody {
    product_id: String,
    #[serde(default)]
    quantity: i32,
}

async fn add_item(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(mut body): Json<ItemBody>,
) -> Result<Json<Value>, ApiError> {
    if body.quantity < 1 {
        body.quantity = 1;
    }
    let cart = ensure_cart(&state, &user).await?;
    if cart.status != CART_OPEN && cart.status != CART_STARTING {
        return Err(ApiError(
            StatusCode::CONFLICT,
            StoreError::CartClosed.to_string(),
        ));
    }
    let product = state.store.get_product(&body.product_id).await?;
    state
        .store
        .reserve(&cart.id, &product.id, body.quantity)
        .await?;
    let mut cart = state.store.get_cart(&cart.id).await?;
    upsert_item(&mut cart.items, &product, body.quantity);
    store::append_cart_event(
        &mut cart,
        "item_added",
        format!("{} x{}", product.name, body.quantity),
    );
    state.store.put_cart(cart.clone()).await?;
    Ok(Json(cart_view(cart)))
}

async fn update_item(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(product_id): Path<String>,
    Json(body): Json<ItemBody>,
) -> Result<Json<Value>, ApiError> {
    let mut cart = current_cart(&state, &user.id, true).await?;
    if cart.status != CART_OPEN && cart.status != CART_STARTING {
        return Err(ApiError(
            StatusCode::CONFLICT,
            StoreError::CartClosed.to_string(),
        ));
    }
    let current = cart.item_qty(&product_id);
    if current == 0 {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            "item não está no carrinho".into(),
        ));
    }
    if body.quantity < 1 {
        return delete_qty(&state, cart, &product_id, current).await;
    }
    let delta = body.quantity - current;
    if delta != 0 {
        state.store.reserve(&cart.id, &product_id, delta).await?;
    }
    set_qty(&mut cart.items, &product_id, body.quantity);
    store::append_cart_event(
        &mut cart,
        "item_updated",
        format!("{product_id} → {}", body.quantity),
    );
    state.store.put_cart(cart.clone()).await?;
    Ok(Json(cart_view(cart)))
}

async fn remove_item(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(product_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let cart = current_cart(&state, &user.id, true).await?;
    if cart.status != CART_OPEN && cart.status != CART_STARTING {
        return Err(ApiError(
            StatusCode::CONFLICT,
            StoreError::CartClosed.to_string(),
        ));
    }
    let current = cart.item_qty(&product_id);
    if current == 0 {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            "item não está no carrinho".into(),
        ));
    }
    delete_qty(&state, cart, &product_id, current).await
}

async fn delete_qty(
    state: &AppState,
    mut cart: Cart,
    product_id: &str,
    qty: i32,
) -> Result<Json<Value>, ApiError> {
    state.store.reserve(&cart.id, product_id, -qty).await?;
    cart.items.retain(|item| item.product_id != product_id);
    store::append_cart_event(&mut cart, "item_removed", product_id);
    state.store.put_cart(cart.clone()).await?;
    Ok(Json(cart_view(cart)))
}

#[derive(Deserialize)]
struct CheckoutBody {
    method: String,
    card: Option<CardData>,
    #[serde(default)]
    simulate: String,
}

async fn checkout(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(body): Json<CheckoutBody>,
) -> Result<(StatusCode, Json<Order>), ApiError> {
    if body.method != PAY_PIX && body.method != PAY_CARD {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "método deve ser pix ou card".into(),
        ));
    }
    let cart = current_cart(&state, &user.id, true).await?;
    if cart.status != CART_OPEN {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "carrinho ainda não está pronto para checkout".into(),
        ));
    }
    if cart.checkout_callback_id.is_empty() {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "sessão durable ainda inicializando, tente de novo".into(),
        ));
    }
    if cart.items.is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "carrinho vazio".into()));
    }
    if body.method == PAY_CARD {
        let card = body.card.as_ref().ok_or_else(|| {
            ApiError(
                StatusCode::BAD_REQUEST,
                "dados do cartão obrigatórios".into(),
            )
        })?;
        if let Err(err) = payment::validate(&payment::Input {
            number: card.number.clone(),
            exp: card.exp.clone(),
            cvc: card.cvc.clone(),
        }) {
            if err != PaymentError::Declined {
                return Err(ApiError(StatusCode::BAD_REQUEST, err.to_string()));
            }
        }
    }

    let now = Utc::now();
    let mut order = Order {
        id: id::new("ord"),
        user_id: user.id,
        cart_id: cart.id.clone(),
        status: ORDER_PENDING_PAYMENT.into(),
        method: body.method.clone(),
        items: cart.items.clone(),
        total_cents: cart.total_cents(),
        currency: crate::domain::CURRENCY_BRL.into(),
        pix: None,
        card_last4: String::new(),
        simulate: body.simulate.clone(),
        events: Vec::new(),
        created_at: now,
        updated_at: now,
    };
    store::append_order_event(
        &mut order,
        "created",
        "pedido criado, aguardando orquestração durable",
    );
    state.store.put_order(order.clone()).await?;

    let signal = CheckoutSignal {
        order_id: order.id.clone(),
        method: body.method,
        card: body.card,
        simulate: body.simulate,
    };
    state
        .orch
        .signal_checkout(&cart.checkout_callback_id, signal)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "signal checkout");
            ApiError(
                StatusCode::BAD_GATEWAY,
                "falha ao sinalizar a função durable".into(),
            )
        })?;

    let fresh = state.store.get_order(&order.id).await.unwrap_or(order);
    Ok((StatusCode::ACCEPTED, Json(fresh)))
}

async fn list_orders(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
) -> Result<Json<Vec<Order>>, ApiError> {
    Ok(Json(state.store.list_orders_by_user(&user.id).await?))
}

async fn get_order(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(id): Path<String>,
) -> Result<Json<Order>, ApiError> {
    let order = state.store.get_order(&id).await?;
    if order.user_id != user.id {
        return Err(StoreError::NotFound.into());
    }
    Ok(Json(order))
}

async fn confirm_pix(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(id): Path<String>,
) -> Result<Json<Order>, ApiError> {
    let (cart, order) = order_cart(&state, &user.id, &id).await?;
    if order.status != ORDER_AWAITING_PIX || cart.pix_callback_id.is_empty() {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "pedido não está aguardando PIX".into(),
        ));
    }
    if order
        .pix
        .as_ref()
        .is_some_and(|p| Utc::now() > p.expires_at)
    {
        return Err(ApiError(StatusCode::CONFLICT, "PIX já expirou".into()));
    }
    state
        .orch
        .signal_pix_paid(&cart.pix_callback_id)
        .await
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e.to_string()))?;
    Ok(Json(
        state.store.get_order(&order.id).await.unwrap_or(order),
    ))
}

async fn expire_pix(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(id): Path<String>,
) -> Result<Json<Order>, ApiError> {
    let (cart, order) = order_cart(&state, &user.id, &id).await?;
    if order.status != ORDER_AWAITING_PIX || cart.pix_callback_id.is_empty() {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "pedido não está aguardando PIX".into(),
        ));
    }
    state
        .orch
        .signal_failure(
            &cart.pix_callback_id,
            "PixExpired",
            "simulação de expiração PIX",
        )
        .await
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e.to_string()))?;
    Ok(Json(
        state.store.get_order(&order.id).await.unwrap_or(order),
    ))
}

async fn order_cart(
    state: &AppState,
    user_id: &str,
    order_id: &str,
) -> Result<(Cart, Order), ApiError> {
    let order = state.store.get_order(order_id).await?;
    if order.user_id != user_id {
        return Err(StoreError::NotFound.into());
    }
    let cart = state.store.get_cart(&order.cart_id).await?;
    Ok((cart, order))
}

async fn ensure_cart(state: &AppState, user: &User) -> Result<Cart, ApiError> {
    match state.store.get_open_cart_by_user(&user.id).await {
        Ok(cart) => return Ok(cart),
        Err(StoreError::NotFound) => {}
        Err(err) => return Err(err.into()),
    }
    let now = Utc::now();
    let mut cart = Cart {
        id: id::new("cart"),
        user_id: user.id.clone(),
        status: CART_STARTING.into(),
        items: Vec::new(),
        execution_arn: String::new(),
        checkout_callback_id: String::new(),
        pix_callback_id: String::new(),
        order_id: String::new(),
        expires_at: None,
        events: Vec::new(),
        created_at: now,
        updated_at: now,
    };
    store::append_cart_event(
        &mut cart,
        "created",
        "carrinho criado, iniciando função durable",
    );
    state.store.put_cart(cart.clone()).await?;
    let arn = state
        .orch
        .start_cart_session(&cart.id)
        .await
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e.to_string()))?;
    if let Ok(mut fresh) = state.store.get_cart(&cart.id).await {
        if fresh.execution_arn.is_empty() {
            fresh.execution_arn = arn;
            let _ = state.store.put_cart(fresh.clone()).await;
        }
        return Ok(fresh);
    }
    Ok(cart)
}

async fn current_cart(state: &AppState, user_id: &str, required: bool) -> Result<Cart, ApiError> {
    match state.store.get_open_cart_by_user(user_id).await {
        Ok(cart) => Ok(cart),
        Err(StoreError::NotFound) if !required => Err(StoreError::NotFound.into()),
        Err(err) => Err(err.into()),
    }
}

async fn require_user(
    State(state): State<AppState>,
    mut req: Request<Body>,
    next: Next,
) -> Result<Response, ApiError> {
    let token = bearer(req.headers())
        .ok_or_else(|| ApiError(StatusCode::UNAUTHORIZED, "faça login como visitante".into()))?;
    let session = match state.store.get_session(&token).await {
        Ok(s) => s,
        Err(_) => return Err(ApiError(StatusCode::UNAUTHORIZED, "sessão inválida".into())),
    };
    let user = match state.store.get_user(&session.user_id).await {
        Ok(u) => u,
        Err(_) => {
            return Err(ApiError(
                StatusCode::UNAUTHORIZED,
                "usuário não encontrado".into(),
            ))
        }
    };
    req.extensions_mut().insert(user);
    Ok(next.run(req).await)
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

fn cart_view(mut cart: Cart) -> Value {
    let total = cart.total_cents();
    cart.checkout_callback_id.clear();
    cart.pix_callback_id.clear();
    let mut value = serde_json::to_value(&cart).unwrap_or_else(|_| json!({}));
    if let Some(obj) = value.as_object_mut() {
        obj.insert("totalCents".into(), json!(total));
    }
    value
}

fn upsert_item(items: &mut Vec<CartItem>, product: &crate::domain::Product, qty: i32) {
    if let Some(item) = items.iter_mut().find(|i| i.product_id == product.id) {
        item.quantity += qty;
        return;
    }
    items.push(CartItem {
        product_id: product.id.clone(),
        name: product.name.clone(),
        emoji: product.emoji.clone(),
        unit_cents: product.price_cents,
        quantity: qty,
    });
}

fn set_qty(items: &mut [CartItem], product_id: &str, qty: i32) {
    if let Some(item) = items.iter_mut().find(|i| i.product_id == product_id) {
        item.quantity = qty;
    }
}
