use anyhow::{Context, Result};
use kiteconnect::ticker::{KiteTicker, KiteTickerHandler, WebSocketHandler};
use serde_json::Value;
use std::collections::VecDeque;
use std::str::FromStr;
use tokio::sync::mpsc;

use crate::live_sma_trader::{Signal, SmaSignalEngine};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PositionStatus {
    Bull,
    Bear,
}

#[derive(Debug, Clone, Copy)]
pub enum Mode {
    Ltp,
    Quote,
    Full,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Ltp => "ltp",
            Mode::Quote => "quote",
            Mode::Full => "full",
        }
    }
}

impl FromStr for Mode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "ltp" => Ok(Mode::Ltp),
            "quote" => Ok(Mode::Quote),
            "full" => Ok(Mode::Full),
            _ => Err(anyhow::anyhow!(
                "TICKER_MODE must be one of: ltp, quote, full"
            )),
        }
    }
}

#[derive(Debug)]
pub struct Handler {
    pub tokens: Vec<u32>,
    pub mode: Mode,

    pub sma_period: usize,
    pub ltp_history: VecDeque<f64>,
    pub position_status: Option<PositionStatus>,
}

impl KiteTickerHandler for Handler {
    fn on_open<T>(&mut self, ws: &mut WebSocketHandler<T>)
    where
        T: KiteTickerHandler,
    {
        // Subscribe on connect (same idea as Python on_connect -> subscribe). [web:1]
        ws.subscribe(self.tokens.clone());
        println!(
            "Connected. Subscribed to {:?} (requested mode: {}).",
            self.tokens,
            self.mode.as_str()
        );
    }

    fn on_ticks<T>(&mut self, _ws: &mut WebSocketHandler<T>, ticks: Vec<Value>)
    where
        T: KiteTickerHandler,
    {
        for tick in ticks {
            let Some(last_price) = tick.get("last_price").and_then(|x| x.as_f64()) else {
                continue;
            };

            // 1) Rolling window of LTPs
            self.ltp_history.push_back(last_price);
            while self.ltp_history.len() > self.sma_period {
                self.ltp_history.pop_front();
            }

            // 2) Compute SMA when we have enough ticks
            if self.ltp_history.len() != self.sma_period {
                continue;
            }

            let sum: f64 = self.ltp_history.iter().sum();
            let sma = sum / (self.sma_period as f64);

            // 3) Initialize state on first SMA
            if self.position_status.is_none() {
                self.position_status = Some(if last_price > sma {
                    PositionStatus::Bull
                } else {
                    PositionStatus::Bear
                });
            }

            // 4) Crossover signals (match your Python logic)
            if last_price > sma && self.position_status == Some(PositionStatus::Bear) {
                self.position_status = Some(PositionStatus::Bull);
                eprintln!(
                    "BULLISH CROSSOVER: BUY SIGNAL! LTP: {:.2} | SMA: {:.2}",
                    last_price, sma
                );
            }

            if last_price < sma && self.position_status == Some(PositionStatus::Bull) {
                self.position_status = Some(PositionStatus::Bear);
                eprintln!(
                    "BEARISH CROSSOVER: SELL SIGNAL! LTP: {:.2} | SMA: {:.2}",
                    last_price, sma
                );
            }

            // 5) Continuous stream log
            let status_str = match self.position_status {
                Some(PositionStatus::Bull) => "BULL",
                Some(PositionStatus::Bear) => "BEAR",
                None => "None",
            };

            println!(
                "LTP: {:.2} | SMA({}): {:.2} | Status: {}",
                last_price, self.sma_period, sma, status_str
            );
        }
    }

    fn on_close<T>(&mut self, _ws: &mut WebSocketHandler<T>)
    where
        T: KiteTickerHandler,
    {
        println!("WebSocket closed.");
    }

    fn on_error<T>(&mut self, _ws: &mut WebSocketHandler<T>)
    where
        T: KiteTickerHandler,
    {
        println!("WebSocket error.");
    }
}

pub fn parse_tokens(env_value: &str) -> Result<Vec<u32>> {
    let mut out = Vec::new();
    for part in env_value.split(',') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        out.push(
            p.parse::<u32>()
                .with_context(|| format!("bad token: {p}"))?,
        );
    }
    if out.is_empty() {
        anyhow::bail!("TICKER_TOKENS is empty");
    }
    Ok(out)
}

pub fn run_ticker_blocking(
    api_key: &str,
    access_token: &str,
    tokens: Vec<u32>,
    mode: Mode,
) -> Result<()> {
    let sma_period: usize = std::env::var("TICKER_SMA_PERIOD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

    let mut ticker = KiteTicker::new(api_key, access_token); // constructor shown in crate README sample. [web:1]

    let handler = Handler {
        tokens,
        mode,
        sma_period,
        ltp_history: VecDeque::with_capacity(sma_period + 1),
        position_status: None,
    };

    ticker.connect(handler, None);

    loop {
        std::thread::park();
    }
}

pub struct SignalHandler {
    pub tokens: Vec<u32>,
    pub engine: SmaSignalEngine,
    pub tx: mpsc::UnboundedSender<Signal>,
}

impl KiteTickerHandler for SignalHandler {
    fn on_open<T>(&mut self, ws: &mut WebSocketHandler<T>)
    where
        T: KiteTickerHandler,
    {
        ws.subscribe(self.tokens.clone());
        println!("WebSocket opened. Subscribed to {:?}", self.tokens);
    }

    fn on_ticks<T>(&mut self, _ws: &mut WebSocketHandler<T>, ticks: Vec<Value>)
    where
        T: KiteTickerHandler,
    {
        for tick in ticks {
            if let Some(sig) = self.engine.on_tick(&tick) {
                let _ = self.tx.send(sig);
            }
        }
    }

    fn on_close<T>(&mut self, _ws: &mut WebSocketHandler<T>)
    where
        T: KiteTickerHandler,
    {
        println!("WebSocket closed.");
    }

    fn on_error<T>(&mut self, _ws: &mut WebSocketHandler<T>)
    where
        T: KiteTickerHandler,
    {
        println!("WebSocket error.");
    }
}

pub fn run_ticker_signals_blocking(
    api_key: &str,
    access_token: &str,
    tokens: Vec<u32>,
    sma_period: usize,
    tx: mpsc::UnboundedSender<Signal>,
) -> Result<(), anyhow::Error> {
    let mut ticker = KiteTicker::new(api_key, access_token);

    let handler = SignalHandler {
        tokens,
        engine: SmaSignalEngine::new(sma_period),
        tx,
    };

    ticker.connect(handler, None);

    loop {
        std::thread::park();
    }
}
