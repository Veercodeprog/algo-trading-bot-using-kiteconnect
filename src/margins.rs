use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

fn auth_headers(
    req: reqwest::RequestBuilder,
    api_key: &str,
    access_token: &str,
) -> reqwest::RequestBuilder {
    req.header("X-Kite-Version", "3")
        .header("Authorization", format!("token {api_key}:{access_token}"))
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderMarginParam<'a> {
    pub exchange: &'a str,
    pub tradingsymbol: &'a str,
    pub transaction_type: &'a str, // "BUY"/"SELL"
    pub variety: &'a str,          // "regular"
    pub product: &'a str,          // "MIS"/"CNC"/"NRML"
    pub order_type: &'a str,       // "MARKET"/"LIMIT"/...
    pub quantity: u32,
    pub price: f64,
    pub trigger_price: f64,
}

pub async fn order_margins(
    api_key: &str,
    access_token: &str,
    orders: &[OrderMarginParam<'_>],
    mode: Option<&str>, // "compact" or None [web:66]
) -> Result<Value> {
    let url = "https://api.kite.trade/margins/orders";

    let req = auth_headers(client().post(url), api_key, access_token).json(orders);
    let req = if let Some(m) = mode {
        req.query(&[("mode", m)])
    } else {
        req
    };

    let resp = req.send().await.context("order_margins request failed")?;
    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .context("order_margins response not JSON")?;

    if !status.is_success() {
        bail!("order_margins failed (HTTP {status}): {body}");
    }
    Ok(body)
}

/// Tries to extract "total" margin for the first order (common usage in examples). [web:66]
pub fn first_total_margin(v: &Value) -> Option<f64> {
    v.get("data")?.get(0)?.get("total")?.as_f64()
}
