use crate::instruments::Instrument;
use chrono::Local;

pub struct FrontFuture {
    pub tradingsymbol: String,
    pub instrument_token: u64,
    pub expiry: String,
}

pub fn find_front_month_nifty_future(instruments: &[Instrument]) -> Option<FrontFuture> {
    let today = Local::now().date_naive();

    // Your instruments struct already has: exchange, name, instrument_type, expiry, tradingsymbol, instrument_token
    let mut futs: Vec<&Instrument> = instruments
        .iter()
        .filter(|x| x.exchange == "NFO")
        .filter(|x| x.name == "NIFTY")
        .filter(|x| x.instrument_type == "FUT")
        .filter(|x| x.expiry.map(|d| d > today).unwrap_or(false))
        .collect();

    futs.sort_by_key(|x| x.expiry);

    let f = futs.first()?;
    Some(FrontFuture {
        tradingsymbol: f.tradingsymbol.clone(),
        instrument_token: f.instrument_token,
        expiry: f
            .expiry
            .map(|d| d.to_string())
            .unwrap_or_else(|| "-".to_string()),
    })
}
