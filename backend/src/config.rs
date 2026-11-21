use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Config {
    pub listen_addr: String,
    pub table_name: String,
    pub durable_function_name: String,
    pub allowed_origin: String,
    pub cart_ttl: Duration,
    pub pix_ttl: Duration,
    pub card_delay: Duration,
    pub card_delay_long: Duration,
    pub session_ttl: Duration,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            listen_addr: env("LISTEN_ADDR", "127.0.0.1:8080"),
            table_name: std::env::var("TABLE_NAME").unwrap_or_default(),
            durable_function_name: std::env::var("DURABLE_FUNCTION_NAME").unwrap_or_default(),
            allowed_origin: env("ALLOWED_ORIGIN", "*"),
            cart_ttl: env_duration("CART_TTL_SECONDS", Duration::from_secs(15 * 60)),
            pix_ttl: env_duration("PIX_TTL_SECONDS", Duration::from_secs(15 * 60)),
            card_delay: env_duration("CARD_DELAY_SECONDS", Duration::from_secs(8)),
            card_delay_long: env_duration("CARD_DELAY_LONG_SECONDS", Duration::from_secs(20)),
            session_ttl: env_duration("SESSION_TTL_SECONDS", Duration::from_secs(7 * 24 * 60 * 60)),
        }
    }

    pub fn is_local(&self) -> bool {
        self.durable_function_name.is_empty()
    }
}

fn env(key: &str, fallback: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| fallback.to_string())
}

fn env_duration(key: &str, fallback: Duration) -> Duration {
    match std::env::var(key) {
        Ok(raw) => raw
            .parse::<u64>()
            .map(Duration::from_secs)
            .unwrap_or(fallback),
        Err(_) => fallback,
    }
}
