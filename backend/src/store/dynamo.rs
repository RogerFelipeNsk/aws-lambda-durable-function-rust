use super::{Store, StoreError};
use crate::domain::{Cart, Order, Product, Session, User};
use async_trait::async_trait;
use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client;
use chrono::Utc;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Clone)]
pub struct Dynamo {
    client: Client,
    table: String,
}

impl Dynamo {
    pub fn new(client: Client, table: impl Into<String>) -> Self {
        Self {
            client,
            table: table.into(),
        }
    }

    async fn put_json<T: Serialize>(
        &self,
        pk: &str,
        sk: &str,
        gsi1pk: &str,
        gsi1sk: &str,
        entity: &str,
        value: &T,
    ) -> Result<(), StoreError> {
        let body = serde_json::to_string(value).map_err(|e| StoreError::Other(e.to_string()))?;
        let mut item = HashMap::from([
            ("pk".into(), AttributeValue::S(pk.into())),
            ("sk".into(), AttributeValue::S(sk.into())),
            ("entity".into(), AttributeValue::S(entity.into())),
            ("body".into(), AttributeValue::S(body)),
        ]);
        if !gsi1pk.is_empty() {
            item.insert("gsi1pk".into(), AttributeValue::S(gsi1pk.into()));
            item.insert("gsi1sk".into(), AttributeValue::S(gsi1sk.into()));
        }
        self.client
            .put_item()
            .table_name(&self.table)
            .set_item(Some(item))
            .send()
            .await
            .map_err(|e| StoreError::Other(e.to_string()))?;
        Ok(())
    }

    async fn get_json<T: DeserializeOwned>(&self, pk: &str, sk: &str) -> Result<T, StoreError> {
        let out = self
            .client
            .get_item()
            .table_name(&self.table)
            .key("pk", AttributeValue::S(pk.into()))
            .key("sk", AttributeValue::S(sk.into()))
            .consistent_read(true)
            .send()
            .await
            .map_err(|e| StoreError::Other(e.to_string()))?;
        let item = out.item.ok_or(StoreError::NotFound)?;
        decode_body(&item)
    }

    async fn query_gsi(
        &self,
        pk: &str,
        sk_prefix: Option<&str>,
    ) -> Result<Vec<HashMap<String, AttributeValue>>, StoreError> {
        let mut items = Vec::new();
        let mut start = None;
        loop {
            let mut req = self
                .client
                .query()
                .table_name(&self.table)
                .index_name("gsi1")
                .expression_attribute_values(":pk", AttributeValue::S(pk.into()));
            if let Some(prefix) = sk_prefix {
                req = req
                    .key_condition_expression("gsi1pk = :pk AND begins_with(gsi1sk, :sk)")
                    .expression_attribute_values(":sk", AttributeValue::S(prefix.into()));
            } else {
                req = req.key_condition_expression("gsi1pk = :pk");
            }
            if let Some(key) = start {
                req = req.set_exclusive_start_key(Some(key));
            }
            let out = req
                .send()
                .await
                .map_err(|e| StoreError::Other(e.to_string()))?;
            items.extend(out.items.unwrap_or_default());
            match out.last_evaluated_key {
                Some(k) => start = Some(k),
                None => break,
            }
        }
        Ok(items)
    }
}

fn decode_body<T: DeserializeOwned>(
    item: &HashMap<String, AttributeValue>,
) -> Result<T, StoreError> {
    let body = item
        .get("body")
        .and_then(|v| v.as_s().ok())
        .ok_or_else(|| StoreError::Other("missing body".into()))?;
    serde_json::from_str(body).map_err(|e| StoreError::Other(e.to_string()))
}

#[async_trait]
impl Store for Dynamo {
    async fn put_product(&self, mut p: Product) -> Result<(), StoreError> {
        if p.reservations.is_empty() {
            p.reservations = HashMap::new();
        }
        self.put_json(
            &format!("PRODUCT#{}", p.id),
            "META",
            "CATALOG",
            &format!("PRODUCT#{}", p.id),
            "product",
            &p,
        )
        .await
    }

    async fn get_product(&self, id: &str) -> Result<Product, StoreError> {
        let mut p: Product = self.get_json(&format!("PRODUCT#{id}"), "META").await?;
        if p.reservations.is_empty() {
            p.reservations = HashMap::new();
        }
        Ok(p)
    }

    async fn list_products(&self) -> Result<Vec<Product>, StoreError> {
        let items = self.query_gsi("CATALOG", None).await?;
        let mut products = Vec::with_capacity(items.len());
        for item in items {
            products.push(decode_body(&item)?);
        }
        Ok(products)
    }

    async fn create_user(&self, u: User) -> Result<(), StoreError> {
        self.put_json(&format!("USER#{}", u.id), "META", "", "", "user", &u)
            .await
    }

    async fn get_user(&self, id: &str) -> Result<User, StoreError> {
        self.get_json(&format!("USER#{id}"), "META").await
    }

    async fn create_session(&self, s: Session) -> Result<(), StoreError> {
        self.put_json(&format!("SESSION#{}", s.id), "META", "", "", "session", &s)
            .await
    }

    async fn get_session(&self, id: &str) -> Result<Session, StoreError> {
        let s: Session = self.get_json(&format!("SESSION#{id}"), "META").await?;
        if Utc::now() > s.expires_at {
            return Err(StoreError::SessionExpired);
        }
        Ok(s)
    }

    async fn put_cart(&self, c: Cart) -> Result<(), StoreError> {
        self.put_json(
            &format!("CART#{}", c.id),
            "META",
            &format!("USER#{}", c.user_id),
            &format!("CART#{}", c.id),
            "cart",
            &c,
        )
        .await
    }

    async fn get_cart(&self, id: &str) -> Result<Cart, StoreError> {
        self.get_json(&format!("CART#{id}"), "META").await
    }

    async fn get_open_cart_by_user(&self, user_id: &str) -> Result<Cart, StoreError> {
        let items = self
            .query_gsi(&format!("USER#{user_id}"), Some("CART#"))
            .await?;
        let mut found: Option<Cart> = None;
        for item in items {
            let c: Cart = decode_body(&item)?;
            if c.is_terminal() {
                continue;
            }
            if found
                .as_ref()
                .is_none_or(|cur| c.created_at > cur.created_at)
            {
                found = Some(c);
            }
        }
        found.ok_or(StoreError::NotFound)
    }

    async fn put_order(&self, o: Order) -> Result<(), StoreError> {
        let sk = format!(
            "ORDER#{}#{}",
            o.created_at
                .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
            o.id
        );
        self.put_json(
            &format!("ORDER#{}", o.id),
            "META",
            &format!("USER#{}", o.user_id),
            &sk,
            "order",
            &o,
        )
        .await
    }

    async fn get_order(&self, id: &str) -> Result<Order, StoreError> {
        self.get_json(&format!("ORDER#{id}"), "META").await
    }

    async fn list_orders_by_user(&self, user_id: &str) -> Result<Vec<Order>, StoreError> {
        let items = self
            .query_gsi(&format!("USER#{user_id}"), Some("ORDER#"))
            .await?;
        let mut orders: Vec<Order> = Vec::with_capacity(items.len());
        for item in items {
            orders.push(decode_body(&item)?);
        }
        orders.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(orders)
    }

    async fn reserve(
        &self,
        cart_id: &str,
        product_id: &str,
        qty: i32,
    ) -> Result<Product, StoreError> {
        if qty == 0 {
            return self.get_product(product_id).await;
        }
        let mut p = self.get_product(product_id).await?;
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
        self.put_product(p.clone()).await?;
        Ok(p)
    }

    async fn release_cart(&self, cart_id: &str) -> Result<(), StoreError> {
        self.mutate_reservations(cart_id, |p, qty| {
            p.available += qty;
            p.reserved -= qty;
            p.reservations.remove(cart_id);
        })
        .await
    }

    async fn commit_cart(&self, cart_id: &str) -> Result<(), StoreError> {
        self.mutate_reservations(cart_id, |p, qty| {
            p.reserved -= qty;
            p.sold += qty;
            p.reservations.remove(cart_id);
        })
        .await
    }
}

impl Dynamo {
    async fn mutate_reservations<F>(&self, cart_id: &str, mut f: F) -> Result<(), StoreError>
    where
        F: FnMut(&mut Product, i32) + Send,
    {
        for p in self.list_products().await? {
            if p.reservations.get(cart_id).copied().unwrap_or(0) == 0 {
                continue;
            }
            let mut fresh = self.get_product(&p.id).await?;
            let held = fresh.reservations.get(cart_id).copied().unwrap_or(0);
            if held == 0 {
                continue;
            }
            f(&mut fresh, held);
            self.put_product(fresh).await?;
        }
        Ok(())
    }
}
