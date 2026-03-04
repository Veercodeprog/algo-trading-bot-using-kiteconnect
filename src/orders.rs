// src/orders.rs
use anyhow::{Context, Result, bail};
use log::info;
use serde_json::Value;
#[derive(Debug, Clone, Copy)]
pub enum OrderType {
    Market,
    Limit,
    Sl,
    Slm, // SL-M
}

impl OrderType {
    fn as_str(self) -> &'static str {
        match self {
            OrderType::Market => "MARKET",
            OrderType::Limit => "LIMIT",
            OrderType::Sl => "SL",
            OrderType::Slm => "SL-M",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    fn as_str(self) -> &'static str {
        match self {
            Side::Buy => "BUY",
            Side::Sell => "SELL",
        }
    }
}

pub async fn place_regular_market_order(
    api_key: &str,
    access_token: &str,
    exchange: &str,      // "NSE"
    tradingsymbol: &str, // "INFY"
    side: Side,          // BUY/SELL
    quantity: u32,       // 1
    product: &str,       // "CNC"
    tag: Option<&str>,   // optional
) -> Result<String> {
    // POST /orders/:variety where variety="regular" [page:0]
    let url = "https://api.kite.trade/orders/regular";

    // Regular order params required by docs (tradingsymbol, exchange, transaction_type, order_type, quantity, product, validity). [page:0]
    let mut form: Vec<(&str, String)> = vec![
        ("tradingsymbol", tradingsymbol.to_string()),
        ("exchange", exchange.to_string()),
        ("transaction_type", side.as_str().to_string()),
        ("order_type", "MARKET".to_string()),
        ("quantity", quantity.to_string()),
        ("product", product.to_string()),
        ("validity", "DAY".to_string()),
    ];

    if let Some(t) = tag {
        form.push(("tag", t.to_string()));
    }

    let client = reqwest::Client::new();
    let resp = client
        .post(url)
        .header("X-Kite-Version", "3")
        .header("Authorization", format!("token {api_key}:{access_token}"))
        .form(&form)
        .send()
        .await
        .context("place_order request failed")?;

    let status = resp.status();
    let body: Value = resp.json().await.context("place_order response not JSON")?;

    if !status.is_success() {
        bail!("place_order failed (HTTP {status}): {body}");
    }

    // Success response returns data.order_id. [page:0]
    let order_id = body
        .get("data")
        .and_then(|d| d.get("order_id"))
        .and_then(|x| x.as_str())
        .context("missing data.order_id in response")?
        .to_string();

    info!(
        "Order placed: side={} exchange={} symbol={} qty={} product={} order_id={}",
        side.as_str(),
        exchange,
        tradingsymbol,
        quantity,
        product,
        order_id
    );

    Ok(order_id)
}
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
pub async fn place_regular_order(
    api_key: &str,
    access_token: &str,
    exchange: &str,
    tradingsymbol: &str,
    side: Side,
    quantity: u32,
    product: &str,              // "MIS"/"CNC"
    order_type: OrderType,      // MARKET/LIMIT/SL/SL-M
    price: Option<f64>,         // required for LIMIT and SL, must be 0 for SL-M
    trigger_price: Option<f64>, // required for SL and SL-M
    tag: Option<&str>,
) -> Result<String> {
    let url = "https://api.kite.trade/orders/regular"; // POST /orders/:variety [web:270]

    let mut form: Vec<(&str, String)> = vec![
        ("tradingsymbol", tradingsymbol.to_string()),
        ("exchange", exchange.to_string()),
        ("transaction_type", side.as_str().to_string()),
        ("quantity", quantity.to_string()),
        ("product", product.to_string()),
        ("order_type", order_type.as_str().to_string()),
        ("validity", "DAY".to_string()),
    ];

    if let Some(p) = price {
        form.push(("price", p.to_string()));
    }
    if let Some(tp) = trigger_price {
        form.push(("trigger_price", tp.to_string()));
    }
    if let Some(t) = tag {
        form.push(("tag", t.to_string()));
    }

    let resp = auth_headers(client().post(url), api_key, access_token)
        .form(&form)
        .send()
        .await
        .context("place_regular_order request failed")?;

    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .context("place_regular_order response not JSON")?;
    if !status.is_success() {
        bail!("place_regular_order failed (HTTP {status}): {body}");
    }

    let order_id = body
        .get("data")
        .and_then(|d| d.get("order_id"))
        .and_then(|x| x.as_str())
        .context("missing data.order_id")?
        .to_string();

    Ok(order_id)
}
/// Stoploss-Market: order_type=SL-M, trigger_price required, price should be 0. [web:270]
pub async fn place_slm_exit_sell(
    api_key: &str,
    access_token: &str,
    exchange: &str,
    tradingsymbol: &str,
    quantity: u32,
    product: &str,
    trigger_price: f64,
    tag: Option<&str>,
) -> Result<String> {
    place_regular_order(
        api_key,
        access_token,
        exchange,
        tradingsymbol,
        Side::Sell,
        quantity,
        product,
        OrderType::Slm,
        Some(0.0),
        Some(trigger_price),
        tag,
    )
    .await
}

/// Stoploss-Limit: order_type=SL, trigger_price + price required. [web:270]
pub async fn place_sl_exit_sell(
    api_key: &str,
    access_token: &str,
    exchange: &str,
    tradingsymbol: &str,
    quantity: u32,
    product: &str,
    trigger_price: f64,
    limit_price: f64,
    tag: Option<&str>,
) -> Result<String> {
    place_regular_order(
        api_key,
        access_token,
        exchange,
        tradingsymbol,
        Side::Sell,
        quantity,
        product,
        OrderType::Sl,
        Some(limit_price),
        Some(trigger_price),
        tag,
    )
    .await
}

// POST /orders/regular (place) [page:0]
pub async fn place_regular_limit_order(
    api_key: &str,
    access_token: &str,
    exchange: &str,      // "NSE"
    tradingsymbol: &str, // "INFY"
    side: Side,          // BUY/SELL
    quantity: u32,
    product: &str, // "CNC"
    price: f64,    // LIMIT price
    tag: Option<&str>,
) -> Result<String> {
    let url = "https://api.kite.trade/orders/regular";

    // Required params depend on order type; for LIMIT we send price. [page:0]
    let mut form: Vec<(&str, String)> = vec![
        ("tradingsymbol", tradingsymbol.to_string()),
        ("exchange", exchange.to_string()),
        ("transaction_type", side.as_str().to_string()),
        ("order_type", "LIMIT".to_string()),
        ("quantity", quantity.to_string()),
        ("product", product.to_string()),
        ("price", price.to_string()),
        ("validity", "DAY".to_string()),
    ];
    if let Some(t) = tag {
        form.push(("tag", t.to_string()));
    }

    let resp = auth_headers(client().post(url), api_key, access_token)
        .form(&form)
        .send()
        .await
        .context("place_regular_limit_order request failed")?;

    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .context("place_regular_limit_order response not JSON")?;
    if !status.is_success() {
        bail!("place_regular_limit_order failed (HTTP {status}): {body}");
    }

    // Success => data.order_id (same idea as official clients) [web:53]
    let order_id = body
        .get("data")
        .and_then(|d| d.get("order_id"))
        .and_then(|x| x.as_str())
        .context("missing data.order_id")?
        .to_string();

    Ok(order_id)
}

// PUT /orders/{variety}/{order_id} (modify) [web:53]
pub async fn modify_order_price(
    api_key: &str,
    access_token: &str,
    variety: &str, // "regular"
    order_id: &str,
    new_price: f64,
) -> Result<String> {
    let url = format!("https://api.kite.trade/orders/{variety}/{order_id}");

    let form = vec![("price", new_price.to_string())];

    let resp = auth_headers(client().put(url), api_key, access_token)
        .form(&form)
        .send()
        .await
        .context("modify_order_price request failed")?;

    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .context("modify_order_price response not JSON")?;
    if !status.is_success() {
        bail!("modify_order_price failed (HTTP {status}): {body}");
    }

    // Most clients return order_id on success. [web:53]
    let oid = body
        .get("data")
        .and_then(|d| d.get("order_id"))
        .and_then(|x| x.as_str())
        .unwrap_or(order_id)
        .to_string();

    Ok(oid)
}

// DELETE /orders/{variety}/{order_id} (cancel) [web:53]
pub async fn cancel_order(
    api_key: &str,
    access_token: &str,
    variety: &str, // "regular"
    order_id: &str,
    parent_order_id: Option<&str>,
) -> Result<String> {
    let url = format!("https://api.kite.trade/orders/{variety}/{order_id}");

    let req = auth_headers(client().delete(url), api_key, access_token);
    let req = if let Some(p) = parent_order_id {
        req.query(&[("parent_order_id", p)])
    } else {
        req
    };

    let resp = req.send().await.context("cancel_order request failed")?;
    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .context("cancel_order response not JSON")?;
    if !status.is_success() {
        bail!("cancel_order failed (HTTP {status}): {body}");
    }

    let oid = body
        .get("data")
        .and_then(|d| d.get("order_id"))
        .and_then(|x| x.as_str())
        .unwrap_or(order_id)
        .to_string();

    Ok(oid)
}

// GET /orders (orderbook) [web:53]
pub async fn orders(api_key: &str, access_token: &str) -> Result<Value> {
    let url = "https://api.kite.trade/orders";
    let resp = auth_headers(client().get(url), api_key, access_token)
        .send()
        .await
        .context("orders request failed")?;

    let status = resp.status();
    let body: Value = resp.json().await.context("orders response not JSON")?;
    if !status.is_success() {
        bail!("orders failed (HTTP {status}): {body}");
    }
    Ok(body)
}

// GET /orders/{order_id} (order history/info) [web:53]
pub async fn order_history(api_key: &str, access_token: &str, order_id: &str) -> Result<Value> {
    let url = format!("https://api.kite.trade/orders/{order_id}");
    let resp = auth_headers(client().get(url), api_key, access_token)
        .send()
        .await
        .context("order_history request failed")?;

    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .context("order_history response not JSON")?;
    if !status.is_success() {
        bail!("order_history failed (HTTP {status}): {body}");
    }
    Ok(body)
}

// GET /trades (tradebook) [web:53]
pub async fn trades(api_key: &str, access_token: &str) -> Result<Value> {
    let url = "https://api.kite.trade/trades";
    let resp = auth_headers(client().get(url), api_key, access_token)
        .send()
        .await
        .context("trades request failed")?;

    let status = resp.status();
    let body: Value = resp.json().await.context("trades response not JSON")?;
    if !status.is_success() {
        bail!("trades failed (HTTP {status}): {body}");
    }
    Ok(body)
}
pub fn extract_order_status(history_json: &serde_json::Value) -> Option<String> {
    // /orders/{order_id} typically returns a list of states; last element is latest.
    // We keep it generic: try array under data or root. [web:53]
    let arr = history_json
        .get("data")
        .and_then(|d| d.as_array())
        .or_else(|| history_json.as_array())?;

    let last = arr.last()?;
    last.get("status")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
}

pub async fn place_order_variety(
    api_key: &str,
    access_token: &str,
    variety: &str, // "regular" | "amo" | "co" | "iceberg"
    exchange: &str,
    tradingsymbol: &str,
    side: Side,
    quantity: u32,
    product: &str,
    order_type: OrderType, // MARKET/LIMIT/SL/SL-M
    price: Option<f64>,
    trigger_price: Option<f64>,
    validity: Option<&str>, // typically "DAY"
    iceberg_legs: Option<u32>,
    iceberg_quantity: Option<u32>,
    tag: Option<&str>,
) -> Result<String> {
    let url = format!("https://api.kite.trade/orders/{variety}");

    let mut form: Vec<(&str, String)> = vec![
        ("tradingsymbol", tradingsymbol.to_string()),
        ("exchange", exchange.to_string()),
        ("transaction_type", side.as_str().to_string()),
        ("order_type", order_type.as_str().to_string()),
        ("quantity", quantity.to_string()),
        ("product", product.to_string()),
        ("validity", validity.unwrap_or("DAY").to_string()),
    ];

    if let Some(p) = price {
        form.push(("price", p.to_string()));
    }
    if let Some(tp) = trigger_price {
        form.push(("trigger_price", tp.to_string()));
    }
    if let Some(l) = iceberg_legs {
        form.push(("iceberg_legs", l.to_string()));
    }
    if let Some(q) = iceberg_quantity {
        form.push(("iceberg_quantity", q.to_string()));
    }
    if let Some(t) = tag {
        form.push(("tag", t.to_string()));
    }

    let resp = auth_headers(client().post(url), api_key, access_token)
        .form(&form)
        .send()
        .await
        .context("place_order_variety request failed")?;

    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .context("place_order_variety response not JSON")?;
    if !status.is_success() {
        bail!("place_order_variety failed (HTTP {status}): {body}");
    }

    let order_id = body
        .get("data")
        .and_then(|d| d.get("order_id"))
        .and_then(|x| x.as_str())
        .context("missing data.order_id")?
        .to_string();

    Ok(order_id)
}
pub async fn place_cover_order_market_buy(
    api_key: &str,
    access_token: &str,
    exchange: &str,
    tradingsymbol: &str,
    quantity: u32,
    product: &str,
    sl_trigger: f64,
    tag: Option<&str>,
) -> Result<String> {
    // CO is an order variety; trigger_price is used for CO/SL orders. [page:0]
    place_order_variety(
        api_key,
        access_token,
        "co",
        exchange,
        tradingsymbol,
        Side::Buy,
        quantity,
        product,
        OrderType::Market,
        Some(0.0),
        Some(sl_trigger),
        Some("DAY"),
        None,
        None,
        tag,
    )
    .await
}

pub async fn place_amo_limit_order(
    api_key: &str,
    access_token: &str,
    exchange: &str,
    tradingsymbol: &str,
    side: Side,
    quantity: u32,
    product: &str,
    price: f64,
    tag: Option<&str>,
) -> Result<String> {
    // AMO is a variety. [page:0]
    place_order_variety(
        api_key,
        access_token,
        "amo",
        exchange,
        tradingsymbol,
        side,
        quantity,
        product,
        OrderType::Limit,
        Some(price),
        None,
        Some("DAY"),
        None,
        None,
        tag,
    )
    .await
}

pub async fn place_iceberg_limit_buy(
    api_key: &str,
    access_token: &str,
    exchange: &str,
    tradingsymbol: &str,
    total_qty: u32,
    product: &str,
    limit_price: f64,
    iceberg_legs: u32,
    iceberg_qty: u32,
    tag: Option<&str>,
) -> Result<String> {
    // Iceberg is a variety and requires iceberg_legs + iceberg_quantity. [page:0]
    place_order_variety(
        api_key,
        access_token,
        "iceberg",
        exchange,
        tradingsymbol,
        Side::Buy,
        total_qty,
        product,
        OrderType::Limit,
        Some(limit_price),
        None,
        Some("DAY"),
        Some(iceberg_legs),
        Some(iceberg_qty),
        tag,
    )
    .await
}
