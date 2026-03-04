use serde_json::Value;

fn obj<'a>(v: &'a Value) -> Option<&'a serde_json::Map<String, Value>> {
    v.as_object()
}

fn f64_at(v: &Value, path: &[&str]) -> Option<f64> {
    let mut cur = v;
    for k in path {
        cur = cur.get(*k)?;
    }
    cur.as_f64()
}

fn i64_at(v: &Value, path: &[&str]) -> Option<i64> {
    let mut cur = v;
    for k in path {
        cur = cur.get(*k)?;
    }
    cur.as_i64()
}

pub fn print_ltp_table(ltp: &Value) {
    println!("\n--- LTP (Last Traded Price) ---");
    println!("{:<18} {:>12}", "Instrument", "LTP");

    if let Some(m) = obj(ltp) {
        // Some client libs return { "status": "...", "data": {...} }
        // Some return directly { "NSE:INFY": {...} }
        let data = m.get("data").unwrap_or(ltp);

        if let Some(dm) = data.as_object() {
            for (k, v) in dm {
                let last = f64_at(v, &["last_price"]).unwrap_or(f64::NAN);
                println!("{:<18} {:>12.2}", k, last);
            }
        }
    }
}

pub fn print_ohlc_table(ohlc: &Value) {
    println!("\n--- OHLC (Open, High, Low, Close) ---");
    println!(
        "{:<18} {:>10} {:>10} {:>10} {:>10} {:>12}",
        "Instrument", "Open", "High", "Low", "Close", "LTP"
    );

    let data = ohlc.get("data").unwrap_or(ohlc);
    if let Some(dm) = data.as_object() {
        for (k, v) in dm {
            let o = f64_at(v, &["ohlc", "open"]).unwrap_or(f64::NAN);
            let h = f64_at(v, &["ohlc", "high"]).unwrap_or(f64::NAN);
            let l = f64_at(v, &["ohlc", "low"]).unwrap_or(f64::NAN);
            let c = f64_at(v, &["ohlc", "close"]).unwrap_or(f64::NAN);
            let ltp = f64_at(v, &["last_price"]).unwrap_or(f64::NAN);
            println!(
                "{:<18} {:>10.2} {:>10.2} {:>10.2} {:>10.2} {:>12.2}",
                k, o, h, l, c, ltp
            );
        }
    }
}

pub fn print_quote_pretty(quote: &Value) {
    println!("\n--- Full Market Quote ---");

    let data = quote.get("data").unwrap_or(quote);
    let Some(dm) = data.as_object() else {
        return;
    };

    for (instrument, v) in dm {
        println!("\n--- Quote for {} ---", instrument);

        let last = f64_at(v, &["last_price"]).unwrap_or(f64::NAN);
        let vol = i64_at(v, &["volume"]).unwrap_or(-1);
        let avg = f64_at(v, &["average_price"]).unwrap_or(f64::NAN);

        let o = f64_at(v, &["ohlc", "open"]).unwrap_or(f64::NAN);
        let h = f64_at(v, &["ohlc", "high"]).unwrap_or(f64::NAN);
        let l = f64_at(v, &["ohlc", "low"]).unwrap_or(f64::NAN);
        let c = f64_at(v, &["ohlc", "close"]).unwrap_or(f64::NAN);

        println!("Last Price: {}", last);
        println!("Volume: {}", vol);
        println!("Average Price: {}", avg);
        println!("OHLC: O={} H={} L={} C={}", o, h, l, c);

        println!("\nMarket Depth (top 5):");
        println!(
            "{:<10} {:>12} | {:<10} {:>12}",
            "BidPx", "BidQty", "AskPx", "AskQty"
        );

        let buys = v
            .get("depth")
            .and_then(|d| d.get("buy"))
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        let sells = v
            .get("depth")
            .and_then(|d| d.get("sell"))
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();

        for i in 0..5 {
            let b = buys.get(i);
            let s = sells.get(i);

            let bpx = b.and_then(|x| x.get("price")).and_then(|x| x.as_f64());
            let bq = b.and_then(|x| x.get("quantity")).and_then(|x| x.as_i64());
            let spx = s.and_then(|x| x.get("price")).and_then(|x| x.as_f64());
            let sq = s.and_then(|x| x.get("quantity")).and_then(|x| x.as_i64());

            println!(
                "{:<10} {:>12} | {:<10} {:>12}",
                bpx.map(|x| format!("{x:.2}")).unwrap_or("-".to_string()),
                bq.map(|x| x.to_string()).unwrap_or("-".to_string()),
                spx.map(|x| format!("{x:.2}")).unwrap_or("-".to_string()),
                sq.map(|x| x.to_string()).unwrap_or("-".to_string()),
            );
        }

        println!("----------------------------------------");
    }
}
