// src/polling_strategy.rs
use crate::{history, instruments};
use anyhow::{Context, Result};
use chrono::{Duration, FixedOffset, Local};

fn sma_of_last(closes: &[f64], period: usize) -> Option<f64> {
    if closes.len() < period {
        return None;
    }
    let slice = &closes[closes.len() - period..];
    Some(slice.iter().sum::<f64>() / period as f64)
}

pub async fn run_lowfreq_sma_loop(
    cfg_api_key: &str,
    access_token: &str,
    instruments_cache: &[instruments::Instrument],
) -> Result<()> {
    let exchange = std::env::var("LOWFREQ_EXCHANGE").unwrap_or_else(|_| "NSE".to_string());
    let symbol = std::env::var("LOWFREQ_SYMBOL").unwrap_or_else(|_| "RELIANCE".to_string());
    let interval = std::env::var("LOWFREQ_INTERVAL").unwrap_or_else(|_| "day".to_string());

    let sma_period: usize = std::env::var("LOWFREQ_SMA_PERIOD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    let lookback_days: i64 = std::env::var("LOWFREQ_LOOKBACK_DAYS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);
    let check_seconds: u64 = std::env::var("LOWFREQ_CHECK_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3600);

    let token: u64 = if let Ok(v) = std::env::var("LOWFREQ_INSTRUMENT_TOKEN") {
        v.parse().context("LOWFREQ_INSTRUMENT_TOKEN must be u64")?
    } else {
        instruments::find_instrument_token(instruments_cache, &exchange, &symbol)
            .context("Could not resolve instrument_token from instruments cache")?
    };

    // IST offset; historical API supports from/to like "yyyy-mm-dd hh:mm:ss". [web:167]
    let ist = FixedOffset::east_opt(5 * 3600 + 30 * 60).unwrap();

    println!(
        "Starting low-frequency SMA loop for {}:{} token={} interval={} SMA({}) every {}s",
        exchange, symbol, token, interval, sma_period, check_seconds
    );

    let mut timer = tokio::time::interval(std::time::Duration::from_secs(check_seconds));
    loop {
        timer.tick().await;

        let to_dt = Local::now().with_timezone(&ist);
        let from_dt = to_dt - Duration::days(lookback_days);

        println!("Running strategy check ({} -> {})", from_dt, to_dt);

        // Fetch candles via Kite historical endpoint. [web:167]
        let candles = history::fetch_historical(
            cfg_api_key,
            access_token,
            token,
            &interval,
            from_dt,
            to_dt,
            false,
            false,
        )
        .await?;

        if candles.len() < sma_period {
            println!(
                "Not enough data for SMA({}). Have {} candles.",
                sma_period,
                candles.len()
            );
            continue;
        }

        let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();
        let sma = sma_of_last(&closes, sma_period).unwrap();
        let last_close = *closes.last().unwrap();

        println!(
            "Data Check | Last Close: {:.2} | SMA({}): {:.2}",
            last_close, sma_period, sma
        );

        // Your “signals” (no orders here by default):
        if last_close > sma {
            println!("POTENTIAL BUY SIGNAL: Last close above SMA");
        } else if last_close < sma {
            println!("POTENTIAL SELL SIGNAL: Last close below SMA");
        }
    }
}
