use crate::domain::{append_event, Cart, Order, Product, Session, User};
use async_trait::async_trait;
use chrono::Utc;
use thiserror::Error;

mod dynamo;
mod memory;

pub use dynamo::Dynamo;
pub use memory::Memory;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("not found")]
    NotFound,
    #[error("conflict")]
    Conflict,
    #[error("estoque insuficiente")]
    InsufficientStock,
    #[error("carrinho não aceita alterações")]
    CartClosed,
    #[error("sessão expirada")]
    SessionExpired,
    #[error("{0}")]
    Other(String),
}

#[async_trait]
pub trait Store: Send + Sync {
    async fn put_product(&self, p: Product) -> Result<(), StoreError>;
    async fn get_product(&self, id: &str) -> Result<Product, StoreError>;
    async fn list_products(&self) -> Result<Vec<Product>, StoreError>;

    async fn create_user(&self, u: User) -> Result<(), StoreError>;
    async fn get_user(&self, id: &str) -> Result<User, StoreError>;

    async fn create_session(&self, s: Session) -> Result<(), StoreError>;
    async fn get_session(&self, id: &str) -> Result<Session, StoreError>;

    async fn put_cart(&self, c: Cart) -> Result<(), StoreError>;
    async fn get_cart(&self, id: &str) -> Result<Cart, StoreError>;
    async fn get_open_cart_by_user(&self, user_id: &str) -> Result<Cart, StoreError>;

    async fn put_order(&self, o: Order) -> Result<(), StoreError>;
    async fn get_order(&self, id: &str) -> Result<Order, StoreError>;
    async fn list_orders_by_user(&self, user_id: &str) -> Result<Vec<Order>, StoreError>;

    async fn reserve(
        &self,
        cart_id: &str,
        product_id: &str,
        qty: i32,
    ) -> Result<Product, StoreError>;
    async fn release_cart(&self, cart_id: &str) -> Result<(), StoreError>;
    async fn commit_cart(&self, cart_id: &str) -> Result<(), StoreError>;
}

pub fn append_cart_event(cart: &mut Cart, kind: &str, message: impl Into<String>) {
    append_event(&mut cart.events, kind, message);
    cart.updated_at = Utc::now();
}

pub fn append_order_event(order: &mut Order, kind: &str, message: impl Into<String>) {
    append_event(&mut order.events, kind, message);
    order.updated_at = Utc::now();
}
