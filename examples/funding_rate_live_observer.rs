//! Funding Rate Live Observer — T29
//!
//! Polls Binance public API for current funding rates and compares to 30-day history.
//! No API keys required — public data.
//!
//! Use as qualitative risk overlay. Extreme funding identifies crowded positioning.
//!
//! Run: cargo run --example funding_rate_live_observer --profile sweep

use anyhow::Result;
use krypto::data::funding_rate::FundingRateLoader;
use polars::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;

const SYMBOLS: &[&str] = &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"];

#[derive(Debug, Deserialize)]
struct PremiumIndex {
    #[serde(rename = "lastFundingRate")]
    last_funding_rate: String,
}

impl PremiumIndex {
    fn funding_rate(&self) -> f64 {
        self.last_funding_rate.parse::<f64>().unwrap_or(0.0)
    }
    fn annualized(&self) -> f64 {
        self.funding_rate() * 3.0 * 365.0
    }
}

struct Stats {
    symbol: String,
    current: f64,
    ann: f64,
    avg30d_ann: f64,
    pctile: f64,
    z: f64,
    mn: f64,
    mx: f64,
    extreme: Extreme,
}

#[derive(Debug, Clone, Copy)]
enum Extreme { None, Bear, Bull, Flip }

impl Extreme {
    fn label(self) -> &'static str {
        match self {
            Extreme::None => "—",
            Extreme::Bear => "BEAR_CROWDED",
            Extreme::Bull => "BULL_CROWDED",
            Extreme::Flip => "FLIP",
        }
    }
}

fn pctile(v: f64, sorted: &[f64]) -> f64 {
    if sorted.is_empty() { return 50.0; }
    let below = sorted.iter().filter(|&&x| x < v).count() as f64;
    (below / sorted.len() as f64) * 100.0
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n=== FUNDING RATE LIVE OBSERVER ===\n");

    let client = reqwest::Client::new();
    let loader = FundingRateLoader::with_cache_dir("examples/funding_cache");

    // ── 1. Fetch current premium index ──────────────────────────────────────
    let mut cur_map: HashMap<String, PremiumIndex> = HashMap::new();
    for &sym in SYMBOLS {
        let url = format!("https://fapi.binance.com/fapi/v1/premiumIndex?symbol={}", sym);
        match client.get(&url).send().await {
            Ok(r) => match r.json::<PremiumIndex>().await {
                Ok(pi) => {
                    let r = pi.funding_rate();
                    let a = pi.annualized();
                    println!("  [OK] {}  rate={:.4}%  ann={:.1}%", sym, r * 100.0, a * 100.0);
                    cur_map.insert(sym.to_string(), pi);
                }
                Err(e) => eprintln!("  [ERR] {} parse: {}", sym, e),
            },
            Err(e) => eprintln!("  [ERR] {} req: {}", sym, e),
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    }

    if cur_map.is_empty() {
        println!("ERROR: no symbols reachable.");
        return Ok(());
    }

    // ── 2. Load 30d history, compute stats ─────────────────────────────────
    let cutoff = chrono::Utc::now().timestamp_millis() - (30 * 24 * 3600 * 1000);
    let mut all_stats: Vec<Stats> = Vec::new();

    for &sym in SYMBOLS {
        let cur = match cur_map.get(sym) {
            Some(c) => c,
            None => continue,
        };

        let hist = match loader.fetch(sym, None, None).await {
            Ok(df) => df,
            Err(_) => { eprintln!("  [WARN] {} no cached history", sym); continue; }
        };

        let times_col = hist.column("time").unwrap().cast(&DataType::Int64).unwrap();
        let times: Vec<Option<i64>> = times_col.i64().unwrap().to_vec();
        let rates_col = hist.column("funding_rate").unwrap().f64().unwrap();
        let rates: Vec<Option<f64>> = rates_col.to_vec();

        let recent: Vec<f64> = times.iter()
            .zip(rates.iter())
            .filter_map(|(&t, &r)| {
                if t.is_some() && r.is_some() && t.unwrap() >= cutoff {
                    Some(r.unwrap())
                } else {
                    None
                }
            })
            .collect();

        if recent.is_empty() {
            eprintln!("  [WARN] {} no data in last 30 days", sym);
            continue;
        }

        let n = recent.len() as f64;
        let avg = recent.iter().sum::<f64>() / n;
        let var = recent.iter().map(|&r| (r - avg).powi(2)).sum::<f64>() / n;
        let std = var.sqrt();
        let mn = recent.iter().cloned().fold(f64::INFINITY, f64::min);
        let mx = recent.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        let cur_r = cur.funding_rate();
        let ann = cur.annualized();
        let pct = pctile(cur_r, &recent);
        let z = if std > 0.0 { (cur_r - avg) / std } else { 0.0 };

        let extreme = if ann < -0.10 {
            Extreme::Bear
        } else if ann > 0.10 {
            Extreme::Bull
        } else if z.abs() > 2.5 {
            Extreme::Flip
        } else {
            Extreme::None
        };

        all_stats.push(Stats {
            symbol: sym.to_string(),
            current: cur_r,
            ann,
            avg30d_ann: avg * 3.0 * 365.0,
            pctile: pct,
            z,
            mn,
            mx,
            extreme,
        });
    }

    // ── 3. Print table ────────────────────────────────────────────────────
    println!("\n{:<10} {:>9} {:>9} {:>9} {:>7} {:>7}  {:>15}  {}",
        "Symbol", "Curr%", "Ann%", "AvgAnn%", "Z", "Pct", "30d Range", "Signal");
    println!("{}", "─".repeat(82));

    for s in &all_stats {
        let rng = format!("[{:.3}%  {:.3}%]", s.mn * 100.0, s.mx * 100.0);
        println!("{:<10} {:>8.4} {:>8.1} {:>8.4} {:>7.2} {:>6.0}  {:>15}  {}",
            s.symbol,
            s.current * 100.0,
            s.ann * 100.0,
            s.avg30d_ann * 100.0,
            s.z,
            s.pctile,
            rng,
            s.extreme.label()
        );
    }

    // ── 4. Summary ────────────────────────────────────────────────────────
    println!("{}", "─".repeat(82));
    if all_stats.is_empty() {
        println!("\nNo stats computed.\n");
        return Ok(());
    }

    let avg_ann: f64 = all_stats.iter().map(|s| s.ann).sum::<f64>() / all_stats.len() as f64;
    let extremes: Vec<_> = all_stats.iter().filter(|s| !matches!(s.extreme, Extreme::None)).collect();

    println!("\n  Base5 avg ann funding: {:.2}%", avg_ann * 100.0);
    if extremes.is_empty() {
        println!("  Extreme signals: NONE");
    } else {
        println!("  Extreme signals ({} of {}):", extremes.len(), all_stats.len());
        for s in &extremes {
            println!("    {}  {:.1}% ann  {}", s.symbol, s.ann * 100.0, s.extreme.label());
        }
    }

    // Interpretation
    println!();
    println!("  REGIME:");
    if avg_ann > 0.10 {
        println!("  Bull market — longs paying. Turtle should be LONG.");
    } else if avg_ann < -0.10 {
        println!("  Bear market — shorts paying. Consider USDT hedge overlay.");
    } else {
        println!("  Neutral. Turtle ATR is primary risk tool.");
    }

    // ── 5. Append CSV log ─────────────────────────────────────────────────
    println!();
    let log = "snapshots/funding_live_observer_log.csv";
    let ts = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
    let is_new = !std::path::Path::new(log).exists();

    let file = std::fs::OpenOptions::new().create(true).append(true).open(log)?;
    let mut wtr = csv::Writer::from_writer(file);
    if is_new {
        wtr.write_record(&["ts","sym","curr","ann","avg30d_ann","pctile","z","mn","mx","extreme"])?;
    }
    for s in &all_stats {
        wtr.write_record(&[
            &ts, &s.symbol,
            &format!("{:.6}", s.current),
            &format!("{:.4}", s.ann * 100.0),
            &format!("{:.4}", s.avg30d_ann * 100.0),
            &format!("{:.2}", s.pctile),
            &format!("{:.3}", s.z),
            &format!("{:.6}", s.mn),
            &format!("{:.6}", s.mx),
            &format!("{:?}", s.extreme),
        ])?;
    }
    wtr.flush()?;
    println!("\n  Logged: {}", log);
    println!();
    Ok(())
}
