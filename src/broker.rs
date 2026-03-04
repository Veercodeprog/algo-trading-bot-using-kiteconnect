// src/broker.rs
use anyhow::{Result, anyhow};
use kiteconnect::connect::KiteConnect;
use serde_json::Value;
pub struct Broker {
    kite: KiteConnect,
}

impl Broker {
    pub fn new(api_key: &str, access_token: &str) -> Self {
        let kite = KiteConnect::new(api_key, access_token);
        Self { kite }
    }

    pub fn profile(&mut self) -> Result<Value> {
        self.kite
            .profile()
            .map_err(|e| anyhow!("profile error: {:?}", e))
    }

    pub fn holdings(&mut self) -> Result<Value> {
        self.kite
            .holdings()
            .map_err(|e| anyhow!("holdings error: {:?}", e))
    }

    pub fn positions(&mut self) -> Result<Value> {
        self.kite
            .positions()
            .map_err(|e| anyhow!("positions error: {:?}", e))
    }

    pub fn orders(&mut self) -> Result<Value> {
        self.kite
            .orders()
            .map_err(|e| anyhow!("orders error: {:?}", e))
    }

    // Optional: margins are in the Kite Connect User API set. [web:7]
    // src/broker.rs

    pub fn margins(&mut self, segment: Option<&str>) -> Result<Value> {
        self.kite
            .margins(segment.map(|s| s.to_string()))
            .map_err(|e| anyhow!("margins error: {:?}", e))
    }
    pub fn ltp(&mut self, instruments: &[&str]) -> Result<Value> {
        self.kite
            .ltp(instruments.to_vec()) // Vec<&str>
            .map_err(|e| anyhow!("ltp error: {:?}", e))
    }

    pub fn ohlc(&mut self, instruments: &[&str]) -> Result<Value> {
        self.kite
            .ohlc(instruments.to_vec()) // Vec<&str>
            .map_err(|e| anyhow!("ohlc error: {:?}", e))
    }

    pub fn quote(&mut self, instruments: &[&str]) -> Result<Value> {
        self.kite
            .quote(instruments.to_vec()) // Vec<&str>
            .map_err(|e| anyhow!("quote error: {:?}", e))
    }

    // If you ever have Vec<String> instruments:
    pub fn ltp_from_strings(&mut self, instruments: &[String]) -> Result<Value> {
        let refs: Vec<&str> = instruments.iter().map(|s| s.as_str()).collect();
        self.kite
            .ltp(refs)
            .map_err(|e| anyhow!("ltp error: {:?}", e))
    }
}
