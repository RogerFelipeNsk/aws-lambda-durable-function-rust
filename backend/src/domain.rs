use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const CART_STARTING: &str = "starting";
pub const CART_OPEN: &str = "open";
pub const CART_CHECKING_OUT: &str = "checking_out";
pub const CART_EXPIRED: &str = "expired";
pub const CART_PAID: &str = "paid";
pub const CART_PAYMENT_FAILED: &str = "payment_failed";
pub const CART_CANCELLED: &str = "cancelled";

pub const ORDER_PENDING_PAYMENT: &str = "pending_payment";
pub const ORDER_AWAITING_PIX: &str = "awaiting_pix";
pub const ORDER_PROCESSING_CARD: &str = "processing_card";
pub const ORDER_PAID: &str = "paid";
pub const ORDER_PIX_EXPIRED: &str = "pix_expired";
pub const ORDER_DECLINED: &str = "declined";
pub const ORDER_CANCELLED: &str = "cancelled";

pub const PAY_PIX: &str = "pix";
pub const PAY_CARD: &str = "card";
pub const CURRENCY_BRL: &str = "BRL";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Product {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub price_cents: i64,
    pub currency: String,
    pub emoji: String,
    pub available: i32,
    pub reserved: i32,
    pub sold: i32,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub reservations: HashMap<String, i32>,
}

impl Product {
    pub fn public(mut self) -> Self {
        self.reservations.clear();
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub name: String,
    pub email: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub user_id: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CartItem {
    pub product_id: String,
    pub name: String,
    pub emoji: String,
    pub unit_cents: i64,
    pub quantity: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimelineEvent {
    pub at: DateTime<Utc>,
    #[serde(rename = "type")]
    pub kind: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cart {
    pub id: String,
    pub user_id: String,
    pub status: String,
    pub items: Vec<CartItem>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub execution_arn: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub checkout_callback_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pix_callback_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub order_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub events: Vec<TimelineEvent>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Cart {
    pub fn total_cents(&self) -> i64 {
        self.items
            .iter()
            .map(|item| item.unit_cents * i64::from(item.quantity))
            .sum()
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status.as_str(),
            CART_EXPIRED | CART_PAID | CART_PAYMENT_FAILED | CART_CANCELLED
        )
    }

    pub fn item_qty(&self, product_id: &str) -> i32 {
        self.items
            .iter()
            .find(|item| item.product_id == product_id)
            .map(|item| item.quantity)
            .unwrap_or(0)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PixCharge {
    pub tx_id: String,
    pub copia_e_cola: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Order {
    pub id: String,
    pub user_id: String,
    pub cart_id: String,
    pub status: String,
    pub method: String,
    pub items: Vec<CartItem>,
    pub total_cents: i64,
    pub currency: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pix: Option<PixCharge>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub card_last4: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub simulate: String,
    #[serde(default)]
    pub events: Vec<TimelineEvent>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckoutSignal {
    pub order_id: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card: Option<CardData>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub simulate: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CardData {
    pub number: String,
    pub exp: String,
    pub cvc: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PixSignal {
    pub paid: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestratorEvent {
    pub cart_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OrchestratorResult {
    pub cart_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub order_id: String,
    pub status: String,
}

pub fn append_event(events: &mut Vec<TimelineEvent>, kind: &str, message: impl Into<String>) {
    events.push(TimelineEvent {
        at: Utc::now(),
        kind: kind.to_string(),
        message: message.into(),
    });
}
