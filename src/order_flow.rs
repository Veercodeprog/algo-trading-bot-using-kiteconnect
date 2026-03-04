// src/order_flow.rs
use anyhow::{Context, Result, anyhow};
use serde_json::Value;

use crate::{broker::Broker, orders};

fn extract_last_price(ltp_json: &Value, key: &str) -> Option<f64> {
    let data = ltp_json.get("data").unwrap_or(ltp_json);
    data.get(key)?.get("last_price")?.as_f64()
}

fn round_1dp(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

fn data_array<'a>(v: &'a Value) -> Option<&'a Vec<Value>> {
    v.get("data")
        .and_then(|d| d.as_array())
        .or_else(|| v.as_array())
}

pub async fn run_limit_manage_demo(
    api_key: &str,
    access_token: &str,
    mut broker: Broker,
) -> Result<()> {
    // Config via env (no cmd prompts)
    let exchange = std::env::var("ORDER_EXCHANGE").unwrap_or_else(|_| "NSE".to_string());
    let symbol = std::env::var("ORDER_SYMBOL").unwrap_or_else(|_| "INFY".to_string());
    let qty: u32 = std::env::var("ORDER_QTY")
        .unwrap_or_else(|_| "1".to_string())
        .parse()?;
    let product = std::env::var("ORDER_PRODUCT").unwrap_or_else(|_| "CNC".to_string());
    let tag = std::env::var("ORDER_TAG").ok();

    let key = format!("{exchange}:{symbol}");

    // 1) LTP -> compute limit price
    let ltp = broker.ltp(&[key.as_str()])?;
    let last =
        extract_last_price(&ltp, &key).ok_or_else(|| anyhow!("missing last_price for {key}"))?;
    let limit_price = round_1dp(last * 0.90);

    log::info!(
        "Current {} LTP is {}. Placing LIMIT BUY at {}",
        symbol,
        last,
        limit_price
    );

    // 2) Place LIMIT BUY (regular)
    let order_id = orders::place_regular_limit_order(
        api_key,
        access_token,
        &exchange,
        &symbol,
        orders::Side::Buy,
        qty,
        &product,
        limit_price,
        tag.as_deref(),
    )
    .await
    .context("placing LIMIT order failed")?;

    log::info!("Sample LIMIT order placed. order_id={}", order_id);

    // 3) Fetch orders (orderbook)
    let ob = orders::orders(api_key, access_token).await?;
    println!("\n--- Orders (selected columns) ---");
    if let Some(rows) = data_array(&ob) {
        println!(
            "{:<22} {:<10} {:<5} {:>4} {:>10} {:<12} {:<18}",
            "order_timestamp", "symbol", "side", "qty", "price", "status", "order_id"
        );
        for r in rows.iter().take(20) {
            let ts = r
                .get("order_timestamp")
                .and_then(|x| x.as_str())
                .unwrap_or("-");
            let sym = r
                .get("tradingsymbol")
                .and_then(|x| x.as_str())
                .unwrap_or("-");
            let side = r
                .get("transaction_type")
                .and_then(|x| x.as_str())
                .unwrap_or("-");
            let qty = r.get("quantity").and_then(|x| x.as_i64()).unwrap_or(0);
            let price = r.get("price").and_then(|x| x.as_f64()).unwrap_or(0.0);
            let status = r.get("status").and_then(|x| x.as_str()).unwrap_or("-");
            let oid = r.get("order_id").and_then(|x| x.as_str()).unwrap_or("-");
            println!(
                "{:<22} {:<10} {:<5} {:>4} {:>10.2} {:<12} {:<18}",
                ts, sym, side, qty, price, status, oid
            );
        }
    } else {
        println!("(No orders array in response; raw={})", ob);
    }

    // 4) Modify price
    let new_limit = round_1dp(last * 0.96);
    orders::modify_order_price(api_key, access_token, "regular", &order_id, new_limit)
        .await
        .context("modify order failed")?;
    log::info!("Modified order {} to new price {}", order_id, new_limit);

    // 5) Cancel
    orders::cancel_order(api_key, access_token, "regular", &order_id, None)
        .await
        .context("cancel order failed")?;
    log::info!("Cancel request placed for {}", order_id);

    // 6) Order history
    let hist = orders::order_history(api_key, access_token, &order_id).await?;
    println!("\n--- Order History (raw) ---\n{}", hist);

    // 7) Trades (tradebook)
    let tr = orders::trades(api_key, access_token).await?;
    println!("\n--- Trades (raw) ---\n{}", tr);

    Ok(())
}
