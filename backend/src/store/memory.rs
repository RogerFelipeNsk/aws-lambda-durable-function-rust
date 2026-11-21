use super::{Store, StoreError};
use crate::domain::{Cart, Order, Product, Session, User};
use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct Memory {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    products: HashMap<String, Product>,
    users: HashMap<String, User>,
    sessions: HashMap<String, Session>,
    carts: HashMap<String, Cart>,
    orders: HashMap<String, Order>,
}

impl Memory {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl Store for Memory {
    async fn put_product(&self, mut p: Product) -> Result<(), StoreError> {
        let mut inner = self.inner.lock().expect("memory store");
        if p.reservations.is_empty() {
            p.reservations = HashMap::new();
        }
        inner.products.insert(p.id.clone(), p);
        Ok(())
    }

    async fn get_product(&self, id: &str) -> Result<Product, StoreError> {
        let inner = self.inner.lock().expect("memory store");
        inner.products.get(id).cloned().ok_or(StoreError::NotFound)
    }

    async fn list_products(&self) -> Result<Vec<Product>, StoreError> {
        let inner = self.inner.lock().expect("memory store");
        let mut out: Vec<_> = inner.products.values().cloned().collect();
        out.sort_by(|a, b| a.category.cmp(&b.category).then(a.name.cmp(&b.name)));
        Ok(out)
    }

    async fn create_user(&self, u: User) -> Result<(), StoreError> {
        let mut inner = self.inner.lock().expect("memory store");
        inner.users.insert(u.id.clone(), u);
        Ok(())
    }

    async fn get_user(&self, id: &str) -> Result<User, StoreError> {
        let inner = self.inner.lock().expect("memory store");
        inner.users.get(id).cloned().ok_or(StoreError::NotFound)
    }

    async fn create_session(&self, s: Session) -> Result<(), StoreError> {
        let mut inner = self.inner.lock().expect("memory store");
        inner.sessions.insert(s.id.clone(), s);
        Ok(())
    }

    async fn get_session(&self, id: &str) -> Result<Session, StoreError> {
        let inner = self.inner.lock().expect("memory store");
        let s = inner
            .sessions
            .get(id)
            .cloned()
            .ok_or(StoreError::NotFound)?;
        if Utc::now() > s.expires_at {
            return Err(StoreError::SessionExpired);
        }
        Ok(s)
    }

    async fn put_cart(&self, c: Cart) -> Result<(), StoreError> {
        let mut inner = self.inner.lock().expect("memory store");
        inner.carts.insert(c.id.clone(), c);
        Ok(())
    }

    async fn get_cart(&self, id: &str) -> Result<Cart, StoreError> {
        let inner = self.inner.lock().expect("memory store");
        inner.carts.get(id).cloned().ok_or(StoreError::NotFound)
    }

    async fn get_open_cart_by_user(&self, user_id: &str) -> Result<Cart, StoreError> {
        let inner = self.inner.lock().expect("memory store");
        inner
            .carts
            .values()
            .filter(|c| c.user_id == user_id && !c.is_terminal())
            .max_by_key(|c| c.created_at)
            .cloned()
            .ok_or(StoreError::NotFound)
    }

    async fn put_order(&self, o: Order) -> Result<(), StoreError> {
        let mut inner = self.inner.lock().expect("memory store");
        inner.orders.insert(o.id.clone(), o);
        Ok(())
    }

    async fn get_order(&self, id: &str) -> Result<Order, StoreError> {
        let inner = self.inner.lock().expect("memory store");
        inner.orders.get(id).cloned().ok_or(StoreError::NotFound)
    }

    async fn list_orders_by_user(&self, user_id: &str) -> Result<Vec<Order>, StoreError> {
        let inner = self.inner.lock().expect("memory store");
        let mut out: Vec<_> = inner
            .orders
            .values()
            .filter(|o| o.user_id == user_id)
            .cloned()
            .collect();
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(out)
    }

    async fn reserve(
        &self,
        cart_id: &str,
        product_id: &str,
        qty: i32,
    ) -> Result<Product, StoreError> {
        let mut inner = self.inner.lock().expect("memory store");
        let p = inner
            .products
            .get_mut(product_id)
            .ok_or(StoreError::NotFound)?;
        if qty == 0 {
            return Ok(p.clone());
        }
        if qty > 0 && p.available < qty {
            return Err(StoreError::InsufficientStock);
        }
        if qty < 0 {
            let held = p.reservations.get(cart_id).copied().unwrap_or(0);
            if held < -qty {
                return Err(StoreError::Conflict);
            }
        }
        p.available -= qty;
        p.reserved += qty;
        let held = p.reservations.entry(cart_id.to_string()).or_insert(0);
        *held += qty;
        if *held == 0 {
            p.reservations.remove(cart_id);
        }
        Ok(p.clone())
    }

    async fn release_cart(&self, cart_id: &str) -> Result<(), StoreError> {
        let mut inner = self.inner.lock().expect("memory store");
        for p in inner.products.values_mut() {
            if let Some(qty) = p.reservations.remove(cart_id) {
                p.available += qty;
                p.reserved -= qty;
            }
        }
        Ok(())
    }

    async fn commit_cart(&self, cart_id: &str) -> Result<(), StoreError> {
        let mut inner = self.inner.lock().expect("memory store");
        for p in inner.products.values_mut() {
            if let Some(qty) = p.reservations.remove(cart_id) {
                p.reserved -= qty;
                p.sold += qty;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reserve_release_commit() {
        let m = Memory::new();
        m.put_product(Product {
            id: "bola".into(),
            name: "Bola".into(),
            description: String::new(),
            category: "futebol".into(),
            price_cents: 100,
            currency: "BRL".into(),
            emoji: String::new(),
            available: 10,
            reserved: 0,
            sold: 0,
            reservations: HashMap::new(),
        })
        .await
        .unwrap();
        m.reserve("cart1", "bola", 3).await.unwrap();
        let got = m.get_product("bola").await.unwrap();
        assert_eq!((got.available, got.reserved), (7, 3));
        assert!(matches!(
            m.reserve("cart1", "bola", 8).await,
            Err(StoreError::InsufficientStock)
        ));
        m.release_cart("cart1").await.unwrap();
        let got = m.get_product("bola").await.unwrap();
        assert_eq!((got.available, got.reserved, got.sold), (10, 0, 0));
        m.reserve("cart1", "bola", 2).await.unwrap();
        m.commit_cart("cart1").await.unwrap();
        let got = m.get_product("bola").await.unwrap();
        assert_eq!((got.available, got.reserved, got.sold), (8, 0, 2));
    }

    #[tokio::test]
    async fn release_is_idempotent() {
        let m = Memory::new();
        m.put_product(Product {
            id: "bola".into(),
            name: "Bola".into(),
            description: String::new(),
            category: "futebol".into(),
            price_cents: 100,
            currency: "BRL".into(),
            emoji: String::new(),
            available: 5,
            reserved: 0,
            sold: 0,
            reservations: HashMap::new(),
        })
        .await
        .unwrap();
        m.reserve("c", "bola", 1).await.unwrap();
        m.release_cart("c").await.unwrap();
        m.release_cart("c").await.unwrap();
        let got = m.get_product("bola").await.unwrap();
        assert_eq!((got.available, got.reserved), (5, 0));
    }
}
