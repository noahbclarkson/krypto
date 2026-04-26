//! =========================================================
//! Pre-2021 Held-Out Validation: CP=7 vs CP=42
//! =========================================================
//!
//! Tests CP=7 (prior default) vs CP=42 (dense sweep winner)
//! on pre-2021 data — data NEVER used in any CP sweep.
//!
//! If CP=42 >= CP=7 on held-out: update CHAND_PERIOD default.
//! If CP=42 < CP=7: keep CP=7.
//!
//! Usage:
//!   cargo run --example held_out_cp42_validation --profile sweep

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

// ── Params ───────────────────────────────────────────────────────────────────
const CANDLES: u32 = 3000;
const TAKER_FEE: f64 = 0.0004;
const EP: usize = 21;
const CHAND_MULT: f64 = 2.30;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.0;
const ATR_ENTRY_MULT: f64 = 0.00;
const HOLD_MAX: usize = 12;
const POSITION_CAP: usize = 3;
const VOL_LOOKBACK: usize = 1;

// Pre-2021 phases (held-out — NEVER used in any hyperopt)
const PRE2021_PHASES: &[(&str, usize, usize)] = &[
    ("P3-2019",  200,  700),  // 2018-2019 bear + recovery
    ("P2-2020",  700, 1100),  // 2020 COVID + early bull
    ("P1-2021", 1100, 1500),  // 2021 bull (pre-May crash)
];

const PRE2021_SYMBOLS: &[&str] = &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"];
const SUMMARY_CSV: &str = "snapshots/cp42_held_out_summary.csv";

// ── Helpers ─────────────────────────────────────────────────────────────────
fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut trs = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c0 = close.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    if trs.is_empty() { return 0.0; }
    trs.iter().sum::<f64>() / period as f64
}

fn turtle_signal(
    close: &[f64], high: &[f64], low: &[f64],
    entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize,
) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    if let Some(&curr_close) = close.get(idx) {
        let breakout = curr_close > max_close;
        if breakout && atr_mult > 0.0 {
            let atr_val = atr_at(high, low, close, atr_period, idx);
            return curr_close >= max_close + atr_mult * atr_val;
        }
        breakout
    } else { false }
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

impl SymData {
    fn new(c: Vec<f64>, h: Vec<f64>, l: Vec<f64>, v: Vec<f64>) -> Self {
        Self { close: c, high: h, low: l, vol: v }
    }
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    cp: usize,
    _phase_name: &str,
    start_bar: usize,
    end_bar: usize,
) -> (usize, f64, f64, f64, f64) {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut daily_rets = Vec::new();
    let mut total_trades = 0usize;
    let mut bar = start_bar;

    while bar < end_bar {
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for (sym, sd) in sym_data {
            if bar >= sd.close.len() { continue; }
            let rol_vol = if bar >= VOL_LOOKBACK {
                sd.vol[bar.saturating_sub(VOL_LOOKBACK)..=bar].iter().sum::<f64>() / VOL_LOOKBACK as f64
            } else { sd.vol[bar] };
            let price = sd.close.get(bar).copied().unwrap_or(0.0);
            let dv = rol_vol * price;
            scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP)
            .map(|(s, _)| s.to_string()).collect();

        if top_syms.is_empty() { equity_curve.push(equity); bar += 1; continue; }

        let mut entered = false;
        'symloop: for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= EP + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, EP, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();
                        let mut highest_high_chand = sd.high[bar];
                        let mut lowest_low_turtle = sd.low[bar];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n - 1);
                        let mut exit_bar = max_bar;
                        for b in (entry_bar_next..=max_bar).rev() {
                            if b >= n { continue; }
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, cp, b);
                            if atr_chand > 0.0 {
                                let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;
                                let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                                if atr_turtle > 0.0 {
                                    let trail_turtle = lowest_low_turtle - TURTLE_ATR_MULT * atr_turtle;
                                    let stop = trail_chand.max(trail_turtle);
                                    if sd.low[b] <= stop { exit_bar = b; break; }
                                }
                            }
                            if sd.high[b] > highest_high_chand { highest_high_chand = sd.high[b]; }
                            if sd.low[b] < lowest_low_turtle { lowest_low_turtle = sd.low[b]; }
                        }
                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let gross_ret = exit / entry - 1.0;
                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;
                            total_trades += 1;
                            equity *= 1.0 + gross_ret;
                            let avg_daily = gross_ret / bars_held as f64;
                            for _ in 0..bars_held { daily_rets.push(avg_daily); }
                            if equity > peak { peak = equity; }
                            equity_curve.push(equity);
                            bar = exit_bar + 1;
                            entered = true;
                            break 'symloop;
                        }
                    }
                }
            }
        }
        if !entered { equity_curve.push(equity); bar += 1; }
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = {
        let mut peak = f64::NEG_INFINITY;
        let mut mdd = 0.0_f64;
        for &e in &equity_curve { if e > peak { peak = e; } let dd = (peak-e)/peak; if dd > mdd { mdd = dd; } }
        mdd * 100.0
    };
    (total_trades, sharpe, ret, max_dd, equity)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    println!("==== PRE-2021 HELD-OUT: CP=7 vs CP=42 ====");
    println!("   Params: EP={}, CM={}, ATR_P={}, HM={}, EM={}",
             EP, CHAND_MULT, TURTLE_ATR_PERIOD, HOLD_MAX, ATR_ENTRY_MULT);

    let loader = DataLoader::new(None, None);
    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    for &sym in PRE2021_SYMBOLS {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => { raw_cache.insert(sym.to_string(), df); }
            Err(e) => eprintln!("  WARNING: {} load failed: {}", sym, e),
        }
    }
    let n = raw_cache.values().map(|df| df.height()).min().unwrap_or(0).min(2800);

    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in PRE2021_SYMBOLS {
        if let Some(df) = raw_cache.get(&sym.to_string()) {
            let n_min = df.height().min(n);
            let close: Vec<f64> = df.column("close")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect();
            let high: Vec<f64> = df.column("high")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect();
            let low:  Vec<f64> = df.column("low")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect();
            let vol:  Vec<f64> = df.column("volume")?.f64()?.into_iter().filter_map(|x| x).take(n_min).collect();
            sym_data_map.insert(sym.to_string(), SymData::new(close, high, low, vol));
        }
    }

    let cps = vec![7usize, 42];
    let mut all_rows = vec!["phase,cp,trades,sharpe,ret_pct,max_dd_pct,equity_final".to_string()];
    let mut results: HashMap<usize, Vec<(String, f64, f64, f64)>> = HashMap::new();

    for &cp in &cps {
        for &(phase_name, start_bar, end_bar) in PRE2021_PHASES {
            let (trades, sharpe, ret, max_dd, equity) =
                run_sim(&sym_data_map, cp, phase_name, start_bar, end_bar.min(n));
            results.entry(cp).or_default().push((phase_name.to_string(), sharpe, ret, max_dd));
            all_rows.push(format!("{},{},{},{:.4},{:.2},{:.2},{:.4}",
                phase_name, cp, trades, sharpe, ret, max_dd, equity));
            println!("  CP={:>2} | {:>8} | trades={:>3} | Sharpe={:>7.3} | Ret={:>8.2}% | DD={:>6.2}%",
                cp, phase_name, trades, sharpe, ret, max_dd);
        }
    }

    let mut f = File::create(SUMMARY_CSV)?;
    for line in &all_rows { writeln!(f, "{}", line)?; }
    println!("\nWrote: {}", SUMMARY_CSV);

    println!("\n{}", "=".repeat(60));
    println!("AGGREGATE COMPARISON (Pre-2021 Held-Out)");
    println!("{}", "=".repeat(60));
    for &cp in &cps {
        let rows = results.get(&cp).unwrap();
        let avg_sharpe = rows.iter().map(|(_, s, _, _)| s).sum::<f64>() / rows.len() as f64;
        let avg_ret    = rows.iter().map(|(_, _, r, _)| r).sum::<f64>() / rows.len() as f64;
        let avg_dd     = rows.iter().map(|(_, _, _, d)| d).sum::<f64>() / rows.len() as f64;
        let phases_pos = rows.iter().filter(|(_, s, _, _)| *s > 0.0).count();
        println!("CP={:>2}: avg Sharpe={:>7.3}, avg Ret={:>8.2}%, avg DD={:>6.2}%, phases positive={}/{}",
            cp, avg_sharpe, avg_ret, avg_dd, phases_pos, rows.len());
    }
    println!("Elapsed: {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}
