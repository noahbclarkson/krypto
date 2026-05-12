//! T97: FRESHNESS_COOLDOWN held-out validation (pre-2021 data)
//!
//! Validates FC=93 vs FC=0 on pre-2021 held-out data.
//! FC=93 promoted to production bot.rs (T96) via in-sample sweep.
//! Pattern to detect: FC=93 is an in-sample artifact (like EP=24, HAP=0.09).
//!
//! Run: cargo run --example t97_fc_held_out --profile sweep

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

use krypto::data::loader::DataLoader;
use krypto::live::config::{
    FRESHNESS_COOLDOWN, HOLD_MAX, TURTLE_ATR_MULT,
    TURTLE_ATR_PERIOD, TURTLE_EP,
};

const CANDLES: u32 = 3000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Runtime::new()?;
    let symbols = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"];

    let mut fc_results: HashMap<usize, Vec<WinResult>> = HashMap::new();
    for &fc in &[0, FRESHNESS_COOLDOWN] {
        fc_results.insert(fc, vec![]);
    }

    for &sym in &symbols {
        let sd = rt.block_on(load_symbol(&DataLoader::new(None, None), sym))?;
        let n = sd.close.len();
        let held_out_start = n / 2;
        let windows = 6;

        for w in 0..windows {
            let test_start = held_out_start + w * 252;
            let test_end = (test_start + 252).min(n);
            if test_end <= test_start { break; }

            for &fc in &[0, FRESHNESS_COOLDOWN] {
                let res = backtest_fc(&sd, test_start, test_end, fc, sym);
                fc_results.get_mut(&fc).unwrap().push(res);
            }
        }
    }

    // Report
    println!("\n=== FRESHNESS_COOLDOWN Held-Out Validation (pre-2021) ===\n");
    let mut csv_lines = vec!["fc,symbol,window,trades,ret_pct,sharpe,max_dd".to_string()];

    for &fc in &[0, FRESHNESS_COOLDOWN] {
        let res = &fc_results[&fc];
        if res.is_empty() { continue; }
        let n = res.len();
        let pass = res.iter().filter(|r| r.sharpe > 0.0).count();
        let avg_s = res.iter().map(|r| r.sharpe).sum::<f64>() / n as f64;
        let avg_r = res.iter().map(|r| r.ret_pct).sum::<f64>() / n as f64;
        let avg_dd = res.iter().map(|r| r.max_dd).sum::<f64>() / n as f64;
        let tot: usize = res.iter().map(|r| r.trades).sum();

        println!("FC={:3}: {:2}/{:2} pass | Sharpe {:+.3} | Return {:+.1} | DD {:.1} | {} trades",
            fc, pass, n, avg_s, avg_r, avg_dd, tot);

        for r in res {
            csv_lines.push(format!("{},{},{},{},{},{:.4},{:.2}", fc, r.sym, r.window, r.trades, r.ret_pct, r.sharpe, r.max_dd));
        }
    }

    File::create("snapshots/t97_fc_held_out.csv")?.write_all(csv_lines.join("\n").as_bytes())?;
    println!("\nCSV: snapshots/t97_fc_held_out.csv");
    Ok(())
}

#[derive(Clone)]
struct SymData { close: Vec<f64>, high: Vec<f64>, low: Vec<f64> }

struct WinResult {
    sym: String,
    window: usize,
    trades: usize,
    ret_pct: f64,
    sharpe: f64,
    max_dd: f64,
}

async fn load_symbol(loader: &DataLoader, sym: &str) -> Result<SymData, Box<dyn std::error::Error>> {
    let df = match loader.fetch_data(sym, "1d", CANDLES).await {
        Ok(df) => df,
        Err(_) => loader.load_from_cache(sym, "1d")?
            .ok_or("no cache")?,
    };
    let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
    let high = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
    let low = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
    Ok(SymData { close, high, low })
}

fn backtest_fc(sd: &SymData, start: usize, end: usize, fc: usize, sym: &str) -> WinResult {
    let ep = TURTLE_EP;
    let ap = TURTLE_ATR_PERIOD;
    let am = TURTLE_ATR_MULT;
    let hm = HOLD_MAX;

    let mut trades_pct = vec![];
    let mut last_exit_bar: HashMap<String, usize> = HashMap::new();
    let mut pos: Option<Pos> = None;

    for i in start..end.min(sd.close.len()) {
        // Exit check — Turtle ATR trailing stop
        if let Some(ref mut p) = pos {
            p.hh = p.hh.max(sd.high[i]);
            if i.saturating_sub(p.entry_bar) >= ap {
                let atr_start = i.saturating_sub(ap);
                let atr: f64 = sd.high[atr_start..i].iter().zip(sd.low[atr_start..i].iter())
                    .map(|(h, l)| h - l).sum::<f64>() / ap as f64;
                let stop = p.hh - am * atr;
                let bars_held = i - p.entry_bar;
                if sd.low[i] <= stop || bars_held >= hm {
                    trades_pct.push((sd.close[i] - p.entry) / p.entry);
                    last_exit_bar.insert("sym".to_string(), i);
                    pos = None;
                }
            }
        }

        // Entry check — Turtle breakout with freshness cooldown
        if pos.is_none() {
            if let Some(&leb) = last_exit_bar.get("sym") {
                if i - leb < fc { continue; }
            }
            if i >= ep {
                let lb = i.saturating_sub(ep);
                let max_c = sd.close[lb..i].iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                if sd.close[i] > max_c {
                    pos = Some(Pos { entry: sd.close[i], entry_bar: i, hh: sd.high[i] });
                }
            }
        }
    }

    let n = trades_pct.len();
    if n < 3 {
        return WinResult { sym: sym.to_string(), window: 0, trades: 0, ret_pct: 0.0, sharpe: 0.0, max_dd: 0.0 };
    }
    let total: f64 = trades_pct.iter().sum();
    let avg = total / n as f64;
    let var: f64 = trades_pct.iter().map(|&t| { let d = t - avg; d * d }).sum::<f64>() / n as f64;
    let std = var.sqrt().max(1e-9);
    let sharpe = (avg / std) * (252_f64.sqrt());

    // MaxDD
    let mut eq = 1.0_f64;
    let mut peak = 1.0_f64;
    let mut max_dd = 0.0_f64;
    for &t in &trades_pct {
        eq *= 1.0 + t;
        peak = peak.max(eq);
        let dd = (eq - peak) / peak;
        if dd < max_dd { max_dd = dd; }
    }

    WinResult { sym: sym.to_string(), window: 0, trades: n, ret_pct: total * 100.0, sharpe, max_dd: max_dd.abs() * 100.0 }
}

struct Pos { entry: f64, entry_bar: usize, hh: f64 }