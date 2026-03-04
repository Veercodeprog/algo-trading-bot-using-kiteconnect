// src/portfolio.rs
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

// GET /portfolio/positions  -> { data: { net: [...], day: [...] } } [web:284]
pub async fn fetch_positions(api_key: &str, access_token: &str) -> Result<Value> {
    let url = "https://api.kite.trade/portfolio/positions";
    let resp = auth_headers(client().get(url), api_key, access_token)
        .send()
        .await
        .context("positions request failed")?;

    let status = resp.status();
    let body: Value = resp.json().await.context("positions response not JSON")?;
    if !status.is_success() {
        bail!("positions failed (HTTP {status}): {body}");
    }
    Ok(body)
}

#[derive(Debug, Clone, Serialize)]
pub struct ConvertPositionParams<'a> {
    pub exchange: &'a str,
    pub tradingsymbol: &'a str,
    pub transaction_type: &'a str, // "BUY"/"SELL" [web:53]
    pub position_type: &'a str,    // "day"/"overnight" [web:53]
    pub quantity: u32,
    pub old_product: &'a str, // "MIS" [web:53]
    pub new_product: &'a str, // "CNC" [web:53]
}

// PUT /portfolio/positions (convert position) [web:53]
pub async fn convert_position(
    api_key: &str,
    access_token: &str,
    p: ConvertPositionParams<'_>,
) -> Result<Value> {
    let url = "https://api.kite.trade/portfolio/positions";

    let resp = auth_headers(client().put(url), api_key, access_token)
        .form(&p)
        .send()
        .await
        .context("convert_position request failed")?;

    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .context("convert_position response not JSON")?;
    if !status.is_success() {
        bail!("convert_position failed (HTTP {status}): {body}");
    }
    Ok(body)
}

fn get_str<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("-")
}
fn get_i64(v: &Value, k: &str) -> i64 {
    v.get(k).and_then(|x| x.as_i64()).unwrap_or(0)
}
fn get_f64(v: &Value, k: &str) -> f64 {
    v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0)
}

pub fn print_positions_table(positions_json: &Value) {
    let data = positions_json.get("data").unwrap_or(positions_json);

    let net = data
        .get("net")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let day = data
        .get("day")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();

    if net.is_empty() {
        println!("--- Net Positions ---\nNo overnight positions.");
    } else {
        println!("--- Net Positions ---");
        println!(
            "{:<10} {:>8} {:>12} {:>12} {:>12}",
            "symbol", "qty", "avg_price", "last_price", "pnl"
        );
        for p in &net {
            println!(
                "{:<10} {:>8} {:>12.2} {:>12.2} {:>12.2}",
                get_str(p, "tradingsymbol"),
                get_i64(p, "quantity"),
                get_f64(p, "average_price"),
                get_f64(p, "last_price"),
                get_f64(p, "pnl"),
            );
        }
    }

    if day.is_empty() {
        println!("\n--- Day Positions ---\nNo intraday positions.");
    } else {
        println!("\n--- Day Positions ---");
        println!(
            "{:<10} {:<6} {:>8} {:>12} {:>12} {:>12}",
            "symbol", "prod", "qty", "avg_price", "last_price", "m2m"
        );
        for p in &day {
            println!(
                "{:<10} {:<6} {:>8} {:>12.2} {:>12.2} {:>12.2}",
                get_str(p, "tradingsymbol"),
                get_str(p, "product"),
                get_i64(p, "quantity"),
                get_f64(p, "average_price"),
                get_f64(p, "last_price"),
                get_f64(p, "m2m"),
            );
        }
    }
}
