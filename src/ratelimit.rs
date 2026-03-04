use anyhow::{Result, anyhow};
use serde_json::Value;
use std::time::Duration;
use tokio::time::sleep;

use crate::broker::Broker;

fn extract_ltp(resp: &Value, instrument: &str) -> Option<f64> {
    // Some client responses are { "data": { "NSE:INFY": {...}}}
    // Others may be directly { "NSE:INFY": {...} }
    let data = resp.get("data").unwrap_or(resp);
    data.get(instrument)
        .and_then(|x| x.get("last_price"))
        .and_then(|x| x.as_f64())
}

pub async fn spam_ltp(mut broker: Broker, instrument: &str, count: u32) -> Result<()> {
    println!("--- Demonstrating Rate Limit Error (anti-pattern) ---");
    let mut error_count = 0u32;

    for i in 0..count {
        let res = broker.ltp(&[instrument]);
        match res {
            Ok(v) => {
                let ltp =
                    extract_ltp(&v, instrument).ok_or_else(|| anyhow!("missing last_price"))?;
                println!(
                    "Request {}: Success - LTP for {} is {}",
                    i + 1,
                    instrument,
                    ltp
                );
            }
            Err(e) => {
                println!("Request {}: FAILED - {}", i + 1, e);
                error_count += 1;
            }
        }
    }

    if error_count > 0 {
        println!(
            "\nAs expected, we hit throttling and got {} errors.",
            error_count
        );
    }

    Ok(())
}

pub async fn throttle_ltp(
    mut broker: Broker,
    instruments: &[String],
    count: u32,
    sleep_ms: u64,
) -> Result<()> {
    println!("--- OPTIMIZED: Using Throttling + batching ---");

    let inst_refs: Vec<&str> = instruments.iter().map(|s| s.as_str()).collect();

    for i in 0..count {
        println!("Request {} (sleep_ms={}):", i + 1, sleep_ms);

        let v = broker.ltp(&inst_refs)?;
        for inst in &inst_refs {
            if let Some(p) = extract_ltp(&v, inst) {
                println!("  > {}: {}", inst, p);
            } else {
                println!("  > {}: -", inst);
            }
        }

        sleep(Duration::from_millis(sleep_ms)).await;
    }

    Ok(())
}
