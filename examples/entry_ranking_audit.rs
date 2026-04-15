//! Entry Ranking Audit — Does volume-ranked top-3 selection add value?
//!
//! Compare:
//!   A) Turtle+Chandelier with volume-ranked top-3 selection (current production)
//!   B) Turtle+Chandelier on each symbol independently (no ranking, trade all)
//!
//! Both use identical Turtle+Chandelier params (EP=21, ATR=25, CHAND=28/2.0)
//! Outcome: per-symbol and aggregate Sharpe, win rate, avg return, drawdown
//!
//! Usage: cargo run --profile sweep --example entry_ranking_audit

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;

const CANDLES: u32 = 5000;
const TAKER_FEE: f64 = 0.001;
const TURTLE_ENTRY: usize = 21;
const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.00;
const TURTLE_ATR_PERIOD: usize = 25;
const TURTLE_ATR_MULT: f64 = 2.00;
const HOLD_MAX: usize = 45;

const SYMBOLS: [&str; 5] = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"];

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut trs = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c0 = *close.get(i.saturating_sub(1)).unwrap_or(&0.0);
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    if trs.is_empty() { return 0.0; }
    trs.iter().sum::<f64>() / period as f64
}

#[derive(Debug, Default)]
struct Results {
    trades: usize,
    wins: usize,
    total_return: f64,
    equity: f64,
    max_dd: f64,
}

impl Results {
    fn new() -> Self { Self::default() }
    fn add_trade(&mut self, ret: f64) {
        self.trades += 1;
        if ret > 0.0 { self.wins += 1; }
        self.total_return += ret;
        let new_equity = self.equity * (1.0 + ret);
        self.max_dd = self.max_dd.max(self.equity - new_equity);
        self.equity = new_equity;
    }
    fn win_rate(&self) -> f64 { self.trades as f64 / self.wins.max(1) as f64 * 100.0 }
    fn avg_return(&self) -> f64 { self.total_return / self.trades.max(1) as f64 }
}

fn run_turtle_for_sym(sd: &SymData, ranked_mode: bool, bar_start: usize, bar_end: usize) -> Results {
    let mut res = Results::new();
    res.equity = 1.0;

    let mut bar = bar_start;
    while bar + 2 < bar_end {
        let entry_ok = if ranked_mode {
            // ranked mode: already selected this bar for entry by DV ranking
            // Just verify this bar qualifies for entry (close > max_close in lookback)
            if bar < TURTLE_ENTRY + 1 { bar += 1; continue; }
            let start = bar + 1 - TURTLE_ENTRY;
            let max_close = sd.close[start..bar].iter().fold(f64::NEG_INFINITY, |m, &c| m.max(c));
            sd.close[bar] > max_close
        } else {
            // unranked: same entry condition (close > max_close)
            if bar < TURTLE_ENTRY + 1 { bar += 1; continue; }
            let start = bar + 1 - TURTLE_ENTRY;
            let max_close = sd.close[start..bar].iter().fold(f64::NEG_INFINITY, |m, &c| m.max(c));
            sd.close[bar] > max_close
        };

        if !entry_ok { bar += 1; continue; }

        let entry_px = sd.close[bar];
        let entry_fee = entry_px * (1.0 - TAKER_FEE);
        let entry_bar = bar;
        let n_bars = sd.close.len();

        let mut highest_chand = sd.high[bar + 1];
        let mut highest_turtle = sd.high[bar + 1];
        let max_bar = (bar + 1 + HOLD_MAX).min(n_bars.saturating_sub(1));
        let mut exit_bar = max_bar;
        let mut exit_reason = "hold_max";

        for b in (bar + 1)..=max_bar {
            highest_chand = highest_chand.max(sd.high[b]);
            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
            let trail_chand = highest_chand - CHAND_MULT * atr_chand;

            highest_turtle = highest_turtle.max(sd.high[b]);
            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
            let trail_turtle = highest_turtle - TURTLE_ATR_MULT * atr_turtle;

            if sd.close[b] < trail_chand {
                exit_bar = b;
                exit_reason = "chandelier";
                break;
            }
            if sd.close[b] < trail_turtle {
                exit_bar = b;
                exit_reason = "turtle_atr";
                break;
            }
        }

        if exit_bar < bar + 1 { exit_bar = bar + 1; }

        let exit_px = *sd.close.get(exit_bar).unwrap_or(&entry_px);
        let exit_fee = exit_px * (1.0 - TAKER_FEE);
        let gross_ret = exit_fee / entry_fee - 1.0;

        res.add_trade(gross_ret);
        bar = exit_bar + 1;
    }

    res
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Loading data...");
    let loader = DataLoader::new(None, None);
    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    for sym in SYMBOLS {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => { raw_cache.insert(sym.to_string(), df); }
            Err(e) => { eprintln!("WARNING: {} failed: {}", sym, e); }
        }
    }

    let min_len = raw_cache.values().map(|df: &DataFrame| df.height()).min().unwrap_or(0).min(2800);

    let mut sym_data: HashMap<String, SymData> = HashMap::new();
    for (sym, df) in &raw_cache {
        let n = min_len;
        macro_rules! col_f64 {
            ($name:expr) => {{
                let chunked = df.column($name)?.f64()?;
                chunked.into_iter().filter_map(|x| x).take(n).collect::<Vec<_>>()
            }};
        }
        sym_data.insert(sym.clone(), SymData {
            close: col_f64!("close"),
            high: col_f64!("high"),
            low: col_f64!("low"),
            vol: col_f64!("volume"),
        });
    }

    println!("\n{}", "=".repeat(72));
    println!("  ENTRY RANKING AUDIT — Does volume-ranked top-3 add value?");
    println!("{}", "=".repeat(72));

    // Method A: ranked top-3 DV selection (current production)
    // Method B: trade each symbol independently (no ranking)

    // First compute per-symbol DV scores at each bar to implement Method A
    // Method A at each bar: rank by DV, take top 3, then run Turtle entry logic

    let symbols: Vec<String> = SYMBOLS.iter().map(|s| s.to_string()).collect();
    let n = min_len;

    // ---- Method A: Ranked top-3 ----
    let mut ranked_equity = 1.0f64;
    let mut ranked_max_dd = 0.0f64;
    let mut ranked_trades = 0usize;
    let mut ranked_wins = 0usize;
    let mut ranked_by_sym: HashMap<String, (usize, usize)> = HashMap::new();
    let mut ranked_by_yr: HashMap<i32, (usize, usize, f64)> = HashMap::new();

    let mut bar = 0usize;
    while bar + 2 < n {
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in &symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0) * sd.close.get(bar).copied().unwrap_or(0.0);
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<&str> = scores.into_iter().take(3).map(|(s, _)| s).collect();
        if top_syms.is_empty() { bar += 1; continue; }

        let mut entered = false;
        for sym in top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar < TURTLE_ENTRY + 1 || bar >= sd.close.len() { continue; }
                let start = bar + 1 - TURTLE_ENTRY;
                let max_close = sd.close[start..bar].iter().fold(f64::NEG_INFINITY, |m, &c| m.max(c));
                if sd.close[bar] <= max_close { continue; }

                let entry_px = sd.close[bar];
                let entry_fee = entry_px * (1.0 - TAKER_FEE);
                let entry_bar = bar;
                let n_bars = sd.close.len();

                let mut highest_chand = sd.high[bar + 1];
                let mut highest_turtle = sd.high[bar + 1];
                let max_bar_exit = (bar + 1 + HOLD_MAX).min(n_bars.saturating_sub(1));
                let mut exit_bar = max_bar_exit;
                let mut exit_reason = "hold_max";

                for b in (bar + 1)..=max_bar_exit {
                    highest_chand = highest_chand.max(sd.high[b]);
                    let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                    let trail_chand = highest_chand - CHAND_MULT * atr_chand;

                    highest_turtle = highest_turtle.max(sd.high[b]);
                    let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                    let trail_turtle = highest_turtle - TURTLE_ATR_MULT * atr_turtle;

                    if sd.close[b] < trail_chand { exit_bar = b; exit_reason = "chandelier"; break; }
                    if sd.close[b] < trail_turtle { exit_bar = b; exit_reason = "turtle_atr"; break; }
                }
                if exit_bar < bar + 1 { exit_bar = bar + 1; }

                let exit_px = *sd.close.get(exit_bar).unwrap_or(&entry_px);
                let exit_fee = exit_px * (1.0 - TAKER_FEE);
                let gross_ret = exit_fee / entry_fee - 1.0;

                let new_equity = ranked_equity * (1.0 + gross_ret);
                ranked_max_dd = ranked_max_dd.max(ranked_equity - new_equity);
                ranked_equity = new_equity;

                ranked_trades += 1;
                if gross_ret > 0.0 { ranked_wins += 1; }

                let e = ranked_by_sym.entry(sym.to_string()).or_insert((0, 0));
                e.0 += 1;
                if gross_ret > 0.0 { e.1 += 1; }

                bar = exit_bar + 1;
                entered = true;
                break;
            }
        }
        if !entered { bar += 1; }
    }

    // ---- Method B: Each symbol independently ----
    let mut unranked_by_sym: HashMap<String, Results> = HashMap::new();
    let mut unranked_trades = 0usize;
    let mut unranked_wins = 0usize;
    let mut unranked_equity = 1.0f64;
    let mut unranked_max_dd = 0.0f64;

    for sym in &symbols {
        if let Some(sd) = sym_data.get(sym) {
            let res = run_turtle_for_sym(sd, false, 0, n);
            unranked_equity *= res.equity;
            unranked_max_dd = unranked_max_dd.max(res.max_dd);
            unranked_trades += res.trades;
            unranked_wins += res.wins;
            unranked_by_sym.insert(sym.clone(), res);
        }
    }

    println!("\n  METHOD A: Volume-ranked top-3 (current production)");
    println!("    Total trades:    {}", ranked_trades);
    println!("    Win rate:       {:.1}%", ranked_wins as f64 / ranked_trades as f64 * 100.0);
    println!("    Final equity:   {:.4}x", ranked_equity);
    println!("    Max drawdown:   {:.2}%", ranked_max_dd * 100.0);
    println!("    Per-symbol trades (wins):");
    for sym in SYMBOLS {
        if let Some((t, w)) = ranked_by_sym.get(sym) {
            if *t > 0 { println!("      {:<10} trades={:3} wins={}", sym, t, w); }
        }
    }

    println!("\n  METHOD B: Each symbol independently (no ranking)");
    println!("    Total trades:    {}", unranked_trades);
    println!("    Win rate:       {:.1}%", unranked_wins as f64 / unranked_trades as f64 * 100.0);
    println!("    Final equity:   {:.4}x", unranked_equity);
    println!("    Max drawdown:   {:.2}%", unranked_max_dd * 100.0);
    println!("    Per-symbol trades (wins / avg_return):");
    for sym in SYMBOLS {
        if let Some(res) = unranked_by_sym.get(sym) {
            if res.trades > 0 {
                println!("      {:<10} trades={:3} wins={} avg_ret={:+.2}%", sym, res.trades, res.wins, res.avg_return() * 100.0);
            }
        }
    }

    println!("\n{}", "=".repeat(72));
    println!("  VERDICT");
    let rank_wr = ranked_wins as f64 / ranked_trades as f64 * 100.0;
    let unrank_wr = unranked_wins as f64 / unranked_trades as f64 * 100.0;
    let rank_annual = (ranked_equity.powf(365.0 / n as f64) - 1.0) * 100.0;
    let unrank_annual = (unranked_equity.powf(365.0 / n as f64) - 1.0) * 100.0;
    let rank_sharpe = rank_annual / (ranked_max_dd * 100.0 * 1.5);
    let unrank_sharpe = unrank_annual / (unranked_max_dd * 100.0 * 1.5);

    println!("    Ranked equity:  {:.1}x | annual: {:+.1}% | Sharpe: {:.2}", ranked_equity, rank_annual, rank_sharpe);
    println!("  Unranked equity:  {:.1}x | annual: {:+.1}% | Sharpe: {:.2}", unranked_equity, unrank_annual, unrank_sharpe);
    println!("  Equity difference: {:.1}% (ranked vs unranked)", (ranked_equity/unranked_equity - 1.0) * 100.0);
    println!("  Trade count difference: {} (ranked) vs {} (unranked)", ranked_trades, unranked_trades);

    if ranked_equity > unranked_equity {
        println!("\n  → Ranking ADDED value. Top-3 DV selection is production-optimal.");
    } else {
        println!("\n  → Ranking DESTROYED value. Unranked is better.");
    }

    // Per-symbol comparison
    println!("\n  Per-symbol comparison (ranked vs unranked avg return):");
    for sym in SYMBOLS {
        let ru = unranked_by_sym.get(sym).map(|r| r.avg_return()).unwrap_or(0.0);
        let (rt, rw) = ranked_by_sym.get(sym).copied().unwrap_or((0, 0));
        let ru_wins = unranked_by_sym.get(sym).map(|r| r.wins).unwrap_or(0);
        let ru_trades = unranked_by_sym.get(sym).map(|r| r.trades).unwrap_or(0);
        if rt > 0 {
            // ranked avg = total ranked return / ranked trades
            // unranked avg is directly available
            println!("    {:<10} unranked_avg={:+.2}% WR={:.0}% ({}/{}) | ranked_trades={} wins={}", sym, ru*100.0, if ru_trades>0{ru_wins as f64/ru_trades as f64*100.0}else{0.0}, ru_wins, ru_trades, rt, rw);
        } else {
            println!("    {:<10} unranked_avg={:+.2}% WR={:.0}% ({}/{}) | ranked_trades=0", sym, ru*100.0, if ru_trades>0{ru_wins as f64/ru_trades as f64*100.0}else{0.0}, ru_wins, ru_trades);
        }
    }

    println!("{}", "=".repeat(72));

    Ok(())
}