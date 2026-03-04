use anyhow::{Context, Result, bail};
use serde::Deserialize;
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

fn data_array(v: &Value) -> Option<&Vec<Value>> {
    v.get("data")
        .and_then(|d| d.as_array())
        .or_else(|| v.as_array())
}

#[derive(Debug, Deserialize)]
pub struct EquityHolding {
    pub tradingsymbol: String,
    pub quantity: i64,
    pub t1_quantity: i64,
    pub average_price: f64,
    pub last_price: f64,
    pub pnl: f64,
}

pub async fn fetch_equity_holdings(
    api_key: &str,
    access_token: &str,
) -> Result<Vec<EquityHolding>> {
    // portfolio.holdings => /portfolio/holdings [web:53]
    let url = "https://api.kite.trade/portfolio/holdings";
    let resp = auth_headers(client().get(url), api_key, access_token)
        .send()
        .await
        .context("equity holdings request failed")?;

    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .context("equity holdings response not JSON")?;
    if !status.is_success() {
        bail!("equity holdings failed (HTTP {status}): {body}");
    }

    let rows = data_array(&body).context("equity holdings missing data array")?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(serde_json::from_value::<EquityHolding>(r.clone()).context("parse holding row")?);
    }
    Ok(out)
}

pub fn print_equity_holdings_table(h: &[EquityHolding]) {
    if h.is_empty() {
        println!("You have no holdings in your account.");
        return;
    }

    println!("--- Portfolio Holdings ---");
    println!(
        "{:<12} {:>6} {:>6} {:>12} {:>12} {:>12}",
        "symbol", "qty", "t1", "avg_price", "last_price", "pnl"
    );

    for x in h {
        println!(
            "{:<12} {:>6} {:>6} {:>12.2} {:>12.2} {:>12.2}",
            x.tradingsymbol, x.quantity, x.t1_quantity, x.average_price, x.last_price, x.pnl
        );
    }
}

pub fn summarize_equity_holdings(h: &[EquityHolding]) -> (f64, f64, f64) {
    // Matches your pandas logic:
    // invested = (t1 + qty) * avg_price
    // current  = (t1 + qty) * last_price
    // pnl      = sum(pnl)
    let mut invested = 0.0;
    let mut current = 0.0;
    let mut pnl = 0.0;

    for x in h {
        let total_qty = (x.t1_quantity + x.quantity) as f64;
        invested += total_qty * x.average_price;
        current += total_qty * x.last_price;
        pnl += x.pnl;
    }

    (invested, current, pnl)
}

// ---- MF holdings ----

#[derive(Debug, Deserialize)]
pub struct MfHolding {
    pub tradingsymbol: Option<String>,
    pub fund: Option<String>,
    pub folio: Option<String>,
    pub quantity: Option<f64>,
    pub average_price: Option<f64>,
    pub last_price: Option<f64>,
    pub last_price_date: Option<String>,
    pub pnl: Option<f64>,
}

pub async fn fetch_mf_holdings(api_key: &str, access_token: &str) -> Result<Vec<MfHolding>> {
    // mf.holdings => /mf/holdings [web:53]
    let url = "https://api.kite.trade/mf/holdings";
    let resp = auth_headers(client().get(url), api_key, access_token)
        .send()
        .await
        .context("mf holdings request failed")?;

    let status = resp.status();
    let body: Value = resp.json().await.context("mf holdings response not JSON")?;
    if !status.is_success() {
        bail!("mf holdings failed (HTTP {status}): {body}");
    }

    let rows = data_array(&body).context("mf holdings missing data array")?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(serde_json::from_value::<MfHolding>(r.clone()).context("parse mf holding row")?);
    }
    Ok(out)
}

pub fn print_mf_holdings_table(h: &[MfHolding]) {
    println!("--- Mutual Fund Holdings ---");
    if h.is_empty() {
        println!("(No MF holdings)");
        return;
    }

    println!(
        "{:<14} {:<28} {:<12} {:>10} {:>10} {:>10} {:>10}",
        "tradingsymbol", "fund", "folio", "qty", "avg", "last", "pnl"
    );

    for x in h {
        println!(
            "{:<14} {:<28} {:<12} {:>10} {:>10} {:>10} {:>10}",
            x.tradingsymbol.as_deref().unwrap_or("-"),
            x.fund.as_deref().unwrap_or("-"),
            x.folio.as_deref().unwrap_or("-"),
            x.quantity
                .map(|v| format!("{v:.3}"))
                .unwrap_or("-".to_string()),
            x.average_price
                .map(|v| format!("{v:.2}"))
                .unwrap_or("-".to_string()),
            x.last_price
                .map(|v| format!("{v:.2}"))
                .unwrap_or("-".to_string()),
            x.pnl.map(|v| format!("{v:.2}")).unwrap_or("-".to_string()),
        );
    }
}
