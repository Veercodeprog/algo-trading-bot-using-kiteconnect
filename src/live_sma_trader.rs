use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::VecDeque;
use tokio::sync::mpsc;

use crate::{margins, orders};

#[derive(Debug, Clone)]
pub enum Signal {
    BullishCross { ltp: f64, sma: f64 },
    BearishCross { ltp: f64, sma: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SideRel {
    Above,
    Below,
}

#[derive(Debug)]
pub struct SmaSignalEngine {
    pub sma_period: usize,
    pub ltp_history: VecDeque<f64>,
    pub last_rel: Option<SideRel>,
}

impl SmaSignalEngine {
    pub fn new(sma_period: usize) -> Self {
        Self {
            sma_period,
            ltp_history: VecDeque::with_capacity(sma_period + 2),
            last_rel: None,
        }
    }

    pub fn on_tick(&mut self, tick: &Value) -> Option<Signal> {
        let ltp = tick.get("last_price")?.as_f64()?;
        self.ltp_history.push_back(ltp);
        while self.ltp_history.len() > self.sma_period {
            self.ltp_history.pop_front();
        }
        if self.ltp_history.len() != self.sma_period {
            return None;
        }

        let sum: f64 = self.ltp_history.iter().sum();
        let sma = sum / (self.sma_period as f64);

        let rel = if ltp >= sma {
            SideRel::Above
        } else {
            SideRel::Below
        };

        let sig = match (self.last_rel, rel) {
            (Some(SideRel::Below), SideRel::Above) => Some(Signal::BullishCross { ltp, sma }),
            (Some(SideRel::Above), SideRel::Below) => Some(Signal::BearishCross { ltp, sma }),
            (None, _) => None,
            _ => None,
        };

        self.last_rel = Some(rel);
        sig
    }
}

pub async fn run_order_executor(
    api_key: String,
    access_token: String,
    mut rx: mpsc::UnboundedReceiver<Signal>,
) -> Result<()> {
    let live_enabled = std::env::var("LIVE_TRADING_ENABLED").unwrap_or_default() == "true";

    let exchange = std::env::var("TRADE_EXCHANGE").unwrap_or_else(|_| "NSE".to_string());
    let symbol = std::env::var("TRADE_SYMBOL").unwrap_or_else(|_| "RELIANCE".to_string());
    let qty: u32 = std::env::var("TRADE_QTY")
        .unwrap_or_else(|_| "1".to_string())
        .parse()?;
    let product = std::env::var("TRADE_PRODUCT").unwrap_or_else(|_| "MIS".to_string());
    let tag = std::env::var("TRADE_TAG").ok();

    let check_margin =
        std::env::var("CHECK_MARGIN_BEFORE_BUY").unwrap_or_else(|_| "false".to_string()) == "true";

    let mut in_long = false;
    let mut last_action_at = std::time::Instant::now() - std::time::Duration::from_secs(60);

    while let Some(sig) = rx.recv().await {
        // Basic spam protection: 2s cooldown
        if last_action_at.elapsed() < std::time::Duration::from_secs(2) {
            continue;
        }

        match sig {
            Signal::BullishCross { ltp, sma } => {
                println!("BUY SIGNAL (cross up): LTP {:.2} SMA {:.2}", ltp, sma);

                if in_long {
                    continue;
                }

                if check_margin {
                    // For MARKET order, docs show price/trigger_price can be 0. [web:66]
                    let p = margins::OrderMarginParam {
                        exchange: &exchange,
                        tradingsymbol: &symbol,
                        transaction_type: "BUY",
                        variety: "regular",
                        product: &product,
                        order_type: "MARKET",
                        quantity: qty,
                        price: 0.0,
                        trigger_price: 0.0,
                    };
                    let m = margins::order_margins(&api_key, &access_token, &[p], Some("compact"))
                        .await?;
                    if let Some(total) = margins::first_total_margin(&m) {
                        println!("Estimated required margin (compact total): {:.2}", total);
                    } else {
                        println!("Margin response (raw): {}", m);
                    }
                }

                if !live_enabled {
                    println!("LIVE_TRADING_ENABLED=false => not placing BUY");
                    last_action_at = std::time::Instant::now();
                    continue;
                }

                let order_id = orders::place_regular_market_order(
                    &api_key,
                    &access_token,
                    &exchange,
                    &symbol,
                    orders::Side::Buy,
                    qty,
                    &product,
                    tag.as_deref(),
                )
                .await
                .context("BUY order placement failed")?;

                println!(
                    "ORDER PLACED: BUY {} {} | order_id={}",
                    qty, symbol, order_id
                );

                // Poll order history as a stand-in for Python on_order_update.
                for _ in 0..10 {
                    let h = orders::order_history(&api_key, &access_token, &order_id).await?;
                    let st =
                        orders::extract_order_status(&h).unwrap_or_else(|| "UNKNOWN".to_string());
                    println!("ORDER UPDATE (polled): order_id={} status={}", order_id, st);
                    if matches!(st.as_str(), "COMPLETE" | "REJECTED" | "CANCELLED") {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }

                in_long = true;
                last_action_at = std::time::Instant::now();
            }

            Signal::BearishCross { ltp, sma } => {
                println!("SELL SIGNAL (cross down): LTP {:.2} SMA {:.2}", ltp, sma);

                if !in_long {
                    continue;
                }

                if !live_enabled {
                    println!("LIVE_TRADING_ENABLED=false => not placing SELL");
                    last_action_at = std::time::Instant::now();
                    continue;
                }

                let order_id = orders::place_regular_market_order(
                    &api_key,
                    &access_token,
                    &exchange,
                    &symbol,
                    orders::Side::Sell,
                    qty,
                    &product,
                    tag.as_deref(),
                )
                .await
                .context("SELL order placement failed")?;

                println!(
                    "ORDER PLACED: SELL {} {} | order_id={}",
                    qty, symbol, order_id
                );

                for _ in 0..10 {
                    let h = orders::order_history(&api_key, &access_token, &order_id).await?;
                    let st =
                        orders::extract_order_status(&h).unwrap_or_else(|| "UNKNOWN".to_string());
                    println!("ORDER UPDATE (polled): order_id={} status={}", order_id, st);
                    if matches!(st.as_str(), "COMPLETE" | "REJECTED" | "CANCELLED") {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }

                in_long = false;
                last_action_at = std::time::Instant::now();
            }
        }
    }

    Ok(())
}
