use anyhow::{Context, Result, bail};
use chrono::{DateTime, Duration, FixedOffset, Local, NaiveDateTime};
use plotters::prelude::{
    BLACK, BLUE, BitMapBackend, ChartBuilder, GREEN, IntoDrawingArea, LineSeries, PathElement, RED,
    WHITE,
};
use std::path::Path as StdPath;

use anyhow::anyhow;
use plotters::coord::types::RangedDateTime;
use plotters::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Debug, Clone, Serialize)]
pub struct Candle {
    pub time: DateTime<FixedOffset>,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: i64,
    pub oi: Option<i64>,
}

fn format_dt(dt: DateTime<FixedOffset>) -> String {
    // Kite historical API accepts "YYYY-MM-DD HH:MM:SS" in query params. [web:167]
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

pub async fn fetch_historical(
    api_key: &str,
    access_token: &str,
    instrument_token: u64,
    interval: &str,
    from_dt: DateTime<FixedOffset>,
    to_dt: DateTime<FixedOffset>,
    continuous: bool,
    oi: bool,
) -> Result<Vec<Candle>> {
    // Endpoint: GET /instruments/historical/:instrument_token/:interval [web:167]
    let url = format!(
        "https://api.kite.trade/instruments/historical/{}/{}",
        instrument_token, interval
    );

    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .header("X-Kite-Version", "3")
        .header("Authorization", format!("token {api_key}:{access_token}"))
        .query(&[
            ("from", format_dt(from_dt)),
            ("to", format_dt(to_dt)),
            ("continuous", if continuous { "1" } else { "0" }.to_string()),
            ("oi", if oi { "1" } else { "0" }.to_string()),
        ])
        .send()
        .await
        .context("historical request failed")?;

    let status = resp.status();
    let v: Value = resp.json().await.context("historical response not JSON")?;

    if !status.is_success() {
        bail!("historical failed (HTTP {status}): {v}");
    }

    let candles = v
        .get("data")
        .and_then(|d| d.get("candles"))
        .and_then(|c| c.as_array())
        .ok_or_else(|| anyhow::anyhow!("missing data.candles in response"))?;

    let mut out = Vec::with_capacity(candles.len());
    for row in candles {
        // Each row is: [time, open, high, low, close, volume, (optional oi)] [web:167]
        let arr = row
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("candle row not array"))?;

        let ts = arr
            .get(0)
            .and_then(|x| x.as_str())
            .ok_or_else(|| anyhow::anyhow!("candle[0] not string"))?;
        let time = DateTime::parse_from_rfc3339(ts)
            .or_else(|_| DateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%z"))
            .context("parse candle timestamp")?;

        let open = arr
            .get(1)
            .and_then(|x| x.as_f64())
            .ok_or_else(|| anyhow::anyhow!("candle open missing"))?;
        let high = arr
            .get(2)
            .and_then(|x| x.as_f64())
            .ok_or_else(|| anyhow::anyhow!("candle high missing"))?;
        let low = arr
            .get(3)
            .and_then(|x| x.as_f64())
            .ok_or_else(|| anyhow::anyhow!("candle low missing"))?;
        let close = arr
            .get(4)
            .and_then(|x| x.as_f64())
            .ok_or_else(|| anyhow::anyhow!("candle close missing"))?;
        let volume = arr
            .get(5)
            .and_then(|x| x.as_i64())
            .ok_or_else(|| anyhow::anyhow!("candle volume missing"))?;
        let oi_v = arr.get(6).and_then(|x| x.as_i64());

        out.push(Candle {
            time,
            open,
            high,
            low,
            close,
            volume,
            oi: oi_v,
        });
    }

    Ok(out)
}

pub fn write_candles_csv(path: &str, candles: &[Candle]) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path).context("create csv writer")?;
    wtr.write_record(["time", "open", "high", "low", "close", "volume", "oi"])
        .context("write csv header")?;

    for c in candles {
        wtr.write_record([
            c.time.to_rfc3339(),
            c.open.to_string(),
            c.high.to_string(),
            c.low.to_string(),
            c.close.to_string(),
            c.volume.to_string(),
            c.oi.map(|x| x.to_string()).unwrap_or_default(),
        ])
        .context("write csv row")?;
    }

    wtr.flush().context("flush csv")?;
    Ok(())
}

// -------- optional plotting (multi-timeframe) --------

pub async fn fetch_mtf_and_plot(
    api_key: &str,
    access_token: &str,
    instrument_token: u64,
    days: i64,
    out_png: &str,
) -> Result<()> {
    use plotters::coord::types::RangedDateTime;
    use plotters::prelude::{
        BLACK, BLUE, BitMapBackend, ChartBuilder, GREEN, LineSeries, PathElement, RED, WHITE,
    };

    let ist = FixedOffset::east_opt(5 * 3600 + 30 * 60).unwrap();
    let to_dt = Local::now().with_timezone(&ist);
    let from_dt = to_dt - Duration::days(days);

    let c5 = fetch_historical(
        api_key,
        access_token,
        instrument_token,
        "5minute",
        from_dt,
        to_dt,
        false,
        false,
    )
    .await?;
    let c15 = fetch_historical(
        api_key,
        access_token,
        instrument_token,
        "15minute",
        from_dt,
        to_dt,
        false,
        false,
    )
    .await?;
    let c60 = fetch_historical(
        api_key,
        access_token,
        instrument_token,
        "60minute",
        from_dt,
        to_dt,
        false,
        false,
    )
    .await?;

    // Pick "last trading day" as the date of the last candle in 5m series
    let last_day = c5
        .last()
        .map(|c| c.time.date_naive())
        .ok_or_else(|| anyhow::anyhow!("no 5m candles"))?;

    let f = |cs: &[Candle]| -> Vec<(NaiveDateTime, f64)> {
        cs.iter()
            .filter(|c| c.time.date_naive() == last_day)
            .map(|c| (c.time.naive_local(), c.close))
            .collect()
    };

    let s5 = f(&c5);
    let s15 = f(&c15);
    let s60 = f(&c60);

    let all = s5.iter().chain(s15.iter()).chain(s60.iter());

    let mut min_t: Option<NaiveDateTime> = None;
    let mut max_t: Option<NaiveDateTime> = None;
    let mut min_p: f64 = f64::INFINITY;
    let mut max_p: f64 = f64::NEG_INFINITY;

    for (t, p) in all {
        min_t = Some(match min_t {
            Some(x) => x.min(*t),
            None => *t,
        });
        max_t = Some(match max_t {
            Some(x) => x.max(*t),
            None => *t,
        });

        min_p = min_p.min(*p);
        max_p = max_p.max(*p);
    }

    let min_t = min_t.ok_or_else(|| anyhow::anyhow!("no candles for last day"))?;
    let max_t = max_t.ok_or_else(|| anyhow::anyhow!("no candles for last day"))?;

    let out_path = StdPath::new(out_png);
    if let Some(parent) = out_path.parent() {
        // If user passes "mtf.png", parent() is Some("") on some platforms; treat as OK.
        if !parent.as_os_str().is_empty() && !parent.exists() {
            bail!("output directory does not exist for {}", parent.display());
        }
    }

    let root = BitMapBackend::new(out_png, (1500, 700)).into_drawing_area();
    root.fill(&WHITE)?;

    let mut chart = ChartBuilder::on(&root)
        .caption(
            format!("Close price on {last_day} (5m vs 15m vs 60m)"),
            ("sans-serif", 30),
        )
        .margin(10)
        .x_label_area_size(40)
        .y_label_area_size(70)
        .build_cartesian_2d(
            RangedDateTime::from(min_t..max_t),
            (min_p * 0.99)..(max_p * 1.01),
        )?;

    chart.configure_mesh().x_labels(10).y_labels(10).draw()?;

    chart
        .draw_series(LineSeries::new(s5, &BLUE))?
        .label("5m close")
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &BLUE));
    chart
        .draw_series(LineSeries::new(s15, &RED))?
        .label("15m close")
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &RED));
    chart
        .draw_series(LineSeries::new(s60, &GREEN))?
        .label("60m close")
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &GREEN));

    chart
        .configure_series_labels()
        .border_style(&BLACK)
        .draw()?;
    root.present()?;
    Ok(())
}

pub fn plot_close_vs_oi_png(candles: &[Candle], out_png: &str, title: &str) -> Result<()> {
    let mut pts_close: Vec<(NaiveDateTime, f64)> = Vec::new();
    let mut pts_oi: Vec<(NaiveDateTime, i64)> = Vec::new();

    for c in candles {
        let t = c.time.naive_local();
        pts_close.push((t, c.close));
        if let Some(oi) = c.oi {
            pts_oi.push((t, oi));
        }
    }

    let x_min = pts_close
        .first()
        .map(|x| x.0)
        .ok_or_else(|| anyhow!("no candles"))?;
    let x_max = pts_close
        .last()
        .map(|x| x.0)
        .ok_or_else(|| anyhow!("no candles"))?;

    let (mut min_close, mut max_close) = (f64::INFINITY, f64::NEG_INFINITY);
    for (_, p) in &pts_close {
        min_close = min_close.min(*p);
        max_close = max_close.max(*p);
    }

    let (mut min_oi, mut max_oi) = (i64::MAX, i64::MIN);
    for (_, oi) in &pts_oi {
        min_oi = min_oi.min(*oi);
        max_oi = max_oi.max(*oi);
    }
    if pts_oi.is_empty() {
        return Err(anyhow!(
            "no OI values in candles (did you call with oi=true?)"
        ));
    }

    let root = BitMapBackend::new(out_png, (1500, 700)).into_drawing_area();
    root.fill(&WHITE)?;

    let mut chart = ChartBuilder::on(&root)
        .caption(title, ("sans-serif", 30))
        .margin(10)
        .x_label_area_size(40)
        .y_label_area_size(70)
        .right_y_label_area_size(70)
        .build_cartesian_2d(
            RangedDateTime::from(x_min..x_max),
            (min_close * 0.99)..(max_close * 1.01),
        )?;

    chart
        .configure_mesh()
        .x_labels(8)
        .y_labels(10)
        .x_label_formatter(&|dt| dt.format("%Y-%m-%d").to_string())
        .y_desc("Close")
        .draw()?;

    // Attach secondary Y axis for OI. [web:254]
    let mut dual = chart.set_secondary_coord(RangedDateTime::from(x_min..x_max), min_oi..max_oi);

    dual.configure_secondary_axes()
        .y_desc("Open Interest")
        .draw()?;

    dual.draw_series(LineSeries::new(pts_close, &BLUE))?
        .label("Close")
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &BLUE));

    dual.draw_secondary_series(LineSeries::new(pts_oi, &RED))?
        .label("OI")
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &RED));

    dual.configure_series_labels().border_style(&BLACK).draw()?;

    root.present()?;
    Ok(())
}
