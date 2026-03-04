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
pub struct GttCondition<'a> {
    pub exchange: &'a str,
    pub tradingsymbol: &'a str,
    pub trigger_values: Vec<f64>,
    pub last_price: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct GttOrder<'a> {
    pub exchange: &'a str,
    pub tradingsymbol: &'a str,
    pub transaction_type: &'a str, // "BUY"/"SELL"
    pub quantity: u32,
    pub order_type: &'a str, // "LIMIT"
    pub product: &'a str,    // "CNC"/"MIS"
    pub price: f64,
}

pub async fn place_gtt(
    api_key: &str,
    access_token: &str,
    gtt_type: &str, // "single" or "two-leg" [page:1]
    condition: &GttCondition<'_>,
    orders: &[GttOrder<'_>],
) -> Result<u64> {
    let url = "https://api.kite.trade/gtt/triggers";

    let condition_json = serde_json::to_string(condition).context("serialize condition")?;
    let orders_json = serde_json::to_string(orders).context("serialize orders")?;

    let resp = auth_headers(client().post(url), api_key, access_token)
        .form(&[
            ("type", gtt_type.to_string()),
            ("condition", condition_json),
            ("orders", orders_json),
        ])
        .send()
        .await
        .context("place_gtt request failed")?;

    let status = resp.status();
    let body: Value = resp.json().await.context("place_gtt response not JSON")?;
    if !status.is_success() {
        bail!("place_gtt failed (HTTP {status}): {body}");
    }

    let id = body
        .get("data")
        .and_then(|d| d.get("trigger_id"))
        .and_then(|x| x.as_u64())
        .context("missing data.trigger_id")?;

    Ok(id)
}
