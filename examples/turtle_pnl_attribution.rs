//! T63: Per-Bar PnL Attribution
//!
//! Critical trust question: is Turtle-only edge from few mega-trends or distributed?
//! - Top-N trade equity decomposition (top-5 vs rest)
//! - Winning vs losing trade distributions
//! - Fee cost as % of gross PnL
//! - Max consecutive losing bars
//! - Win rate, avg win/loss ratio
//!
//! Data source: uses turtle_only_equity.csv (from T59) which has:
//!   date, equity, daily_return, cumulative_trades
//! We reconstruct individual trade PnL from the equity curve.

use anyhow::Result;
use chrono::{Datelike, NaiveDate, Utc};
use krypto::data::loader::DataLoader;
use std::collections::VecDeque;
use std::fs::File;
use std::io::Write;

const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const HOLD_MAX: usize = 12;
const POSITION_CAP: usize = 3;
const VOL_LOOKBACK: usize = 96;
const TAKER_FEE: f64 = 0.001;
const REGIME_ATR_PERIOD: usize = 17;
const REGIME_LOOKBACK: usize = 42;
const ATR_RANK_THRESHOLD: f64 = 5.0;

const CANDLES: u32 = 3000;
const WARMUP_BARS: usize = 300;

const BASE_SYMBOLS: [&str; 6] = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"];

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    open: Vec<f64>,
    vol: Vec<f64>,
    dt: Vec<i64>,
}

impl SymData {
    fn trend_signal(&self, idx: usize, ep: usize, regime_ok: bool) -> bool {
        if idx < ep || !regime_ok {
            return false;
        }
        let start = idx - ep;
        let current_close = self.close[idx];
        let mut max_close = self.close[start];
        for &c in &self.close[start..idx] {
            if c > max_close {
                max_close = c;
            }
        }
        current_close > max_close
    }

    fn turtle_exit(
        &self,
        idx: usize,
        entry_idx: usize,
        entry_price: f64,
        atr_period: usize,
        atr_mult: f64,
        hold_max: usize,
    ) -> Option<(f64, i64)> {
        let bars_held = idx - entry_idx;
        if bars_held > hold_max {
            let exit_price = self.close[idx];
            return Some((exit_price, self.dt[idx]));
        }
        let lookback = atr_period.min(idx.saturating_sub(entry_idx));
        if lookback == 0 {
            return None;
        }
        let start = idx - lookback;
        let mut max_high = self.high[start];
        for &h in &self.high[start..=idx] {
            if h > max_high {
                max_high = h;
            }
        }
        let mut atr: f64 = 0.0;
        for k in 0..lookback {
            let h = self.high[idx - k];
            let l = self.low[idx - k];
            atr += (h - l) / lookback as f64;
        }
        let stop = max_high - atr_mult * atr;
        if self.close[idx] < stop {
            return Some((self.close[idx], self.dt[idx]));
        }
        None
    }
}

struct Trade {
    symbol: String,
    entry_date: i64,
    exit_date: i64,
    entry_price: f64,
    exit_price: f64,
    gross_pnl: f64,
    fee: f64,
    net_pnl: f64,
    bars_held: i32,
}

fn fee_for_side(price: f64) -> f64 {
    price * TAKER_FEE
}

fn run_symbol(symbol: &str, warmup: usize, regime_data: &[f64]) -> Result<Vec<Trade>> {
    let loader = DataLoader::new();
    let mut sym = SymData {
        close: Vec::new(),
        high: Vec::new(),
        low: Vec::new(),
        open: Vec::new(),
        vol: Vec::new(),
        dt: Vec::new(),
    };
    loader.load_data_into(
        symbol,
        CANDLES,
        &mut sym.close,
        &mut sym.high,
        &mut sym.low,
        &mut sym.open,
        &mut sym.vol,
        &mut sym.dt,
    )?;
    let n = sym.close.len();
    if n < warmup + TURTLE_ENTRY + 10 {
        return Ok(Vec::new());
    }

    let mut trades = Vec::new();
    let mut position_count = 0usize;
    let mut entry_info: Option<(usize, f64, i64)> = None;

    for idx in warmup..n {
        let regime_ok = regime_data.get(idx).copied().unwrap_or(1.0) >= ATR_RANK_THRESHOLD;

        if let Some((e_idx, e_px, e_dt)) = entry_info {
            if let Some((ex_px, ex_dt)) =
                sym.turtle_exit(idx, e_idx, e_px, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, HOLD_MAX)
            {
                let entry_fee = fee_for_side(e_px);
                let exit_fee = fee_for_side(ex_px);
                let total_fee = entry_fee + exit_fee;
                let gross = ex_px / e_px - 1.0;
                let net = gross - total_fee / e_px;
                trades.push(Trade {
                    symbol: symbol.to_string(),
                    entry_date: e_dt,
                    exit_date: ex_dt,
                    entry_price: e_px,
                    exit_price: ex_px,
                    gross_pnl: gross,
                    fee: total_fee / e_px,
                    net_pnl: net,
                    bars_held: (idx - e_idx) as i32,
                });
                position_count -= 1;
                entry_info = None;
            }
        }

        if entry_info.is_none() && position_count < POSITION_CAP {
            if sym.trend_signal(idx, TURTLE_ENTRY, regime_ok) {
                entry_info = Some((idx, sym.close[idx], sym.dt[idx]));
                position_count += 1;
            }
        }
    }

    Ok(trades)
}

fn regime_btc(warmup: usize, n_btc: usize) -> Vec<f64> {
    let mut close = Vec::new();
    let mut high = Vec::new();
    let mut low = Vec::new();
    let mut open = Vec::new();
    let mut vol = Vec::new();
    let mut dt = Vec::new();
    let loader = DataLoader::new();
    loader.load_data_into(
        "BTCUSDT",
        CANDLES,
        &mut close,
        &mut high,
        &mut low,
        &mut open,
        &mut vol,
        &mut dt,
    ).expect("BTC data required for regime");
    let n = close.len();

    let mut regime = vec![0.0_f64; n];
    for i in REGIME_LOOKBACK..n {
        let mut atr_vals: Vec<f64> = Vec::new();
        for t in (i.saturating_sub(REGIME_ATR_PERIOD)..=i).min_by(|a, b| a.cmp(b)) {
            if t < i {
                atr_vals.push(high[t] - low[t]);
            }
        }
        let cur_lookback = (i.saturating_sub(REGIME_ATR_PERIOD)..i).count().min(REGIME_ATR_PERIOD);
        if cur_lookback == 0 { continue; }
        let cur_atr = atr_vals.last().copied().unwrap_or(0.0);
        if cur_atr <= 0.0 { regime[i] = 0.0; continue; }
        let mut hist: Vec<f64> = Vec::new();
        let start = i.saturating_sub(REGIME_LOOKBACK);
        for k in start..i {
            if k >= cur_lookback {
                hist.push(high[k] - low[k]);
            }
        }
        if hist.is_empty() { regime[i] = 0.0; continue; }
        let rank = hist.iter().filter(|&&x| x < cur_atr).count() as f64 / hist.len() as f64 * 100.0;
        regime[i] = rank;
    }

    if n_btc > regime.len() {
        regime.resize(n_btc, 0.0);
    }
    regime
}

fn main() -> Result<()> {
    println!("=== T63: Per-Bar PnL Attribution ===\n");

    // Compute BTC regime for entry gating
    let loader = DataLoader::new();
    let mut tmp_close = Vec::new();
    let mut tmp_high = Vec::new();
    let mut tmp_low = Vec::new();
    let mut tmp_open = Vec::new();
    let mut tmp_vol = Vec::new();
    let mut tmp_dt = Vec::new();
    loader.load_data_into("BTCUSDT", CANDLES, &mut tmp_close, &mut tmp_high, &mut tmp_low, &mut tmp_open, &mut tmp_vol, &mut tmp_dt)?;
    let n_btc = tmp_close.len();
    let regime = regime_btc(WARMUP_BARS, n_btc);

    let mut all_trades = Vec::new();
    for &sym in &BASE_SYMBOLS {
        let trades = run_symbol(sym, WARMUP_BARS, &regime)?;
        all_trades.extend(trades);
    }

    all_trades.sort_by_key(|t| t.entry_date);

    // ── Core metrics ────────────────────────────────────────────────────
    let n = all_trades.len();
    let gross_sum: f64 = all_trades.iter().map(|t| t.gross_pnl).sum();
    let fee_sum: f64 = all_trades.iter().map(|t| t.fee).sum();
    let net_sum: f64 = all_trades.iter().map(|t| t.net_pnl).sum();
    let wins: usize = all_trades.iter().filter(|t| t.net_pnl > 0.0).count();
    let losses: usize = n - wins;
    let win_rate = if n > 0 { wins as f64 / n as f64 } else { 0.0 };

    let avg_win = if wins > 0 {
        all_trades.iter().filter(|t| t.net_pnl > 0.0).map(|t| t.net_pnl).sum::<f64>() / wins as f64
    } else { 0.0 };
    let avg_loss = if losses > 0 {
        all_trades.iter().filter(|t| t.net_pnl <= 0.0).map(|t| t.net_pnl).sum::<f64>() / losses as f64
    } else { 0.0 };
    let avg_win_loss_ratio = if avg_loss.abs() > 1e-9 { avg_win / avg_loss.abs() } else { 0.0 };

    let fee_pct_of_gross = if gross_sum.abs() > 1e-9 { fee_sum / gross_sum.abs() * 100.0 } else { 0.0 };

    // ── Equity decomposition ────────────────────────────────────────────
    let mut with_pnl: Vec<(usize, f64)> = all_trades
        .iter()
        .enumerate()
        .map(|(i, t)| (i, t.net_pnl))
        .collect();
    with_pnl.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let top5_pct = if with_pnl.len() >= 5 {
        let top5: f64 = with_pnl[0..5].iter().map(|(_, p)| p).sum();
        let total: f64 = with_pnl.iter().map(|(_, p)| p).sum();
        top5 / total.abs() * 100.0
    } else { 100.0 };

    let top10_pct = if with_pnl.len() >= 10 {
        let top10: f64 = with_pnl[0..10].iter().map(|(_, p)| p).sum();
        let total: f64 = with_pnl.iter().map(|(_, p)| p).sum();
        top10 / total.abs() * 100.0
    } else { top5_pct };

    let top20_pct = if with_pnl.len() >= 20 {
        let top20: f64 = with_pnl[0..20].iter().map(|(_, p)| p).sum();
        let total: f64 = with_pnl.iter().map(|(_, p)| p).sum();
        top20 / total.abs() * 100.0
    } else { top10_pct };

    // ── Consecutive losing bars ───────────────────────────────────────────
    let loader2 = DataLoader::new();
    let mut btc_close = Vec::new();
    let mut btc_high = Vec::new();
    let mut btc_low = Vec::new();
    let mut btc_open = Vec::new();
    let mut btc_vol = Vec::new();
    let mut btc_dt = Vec::new();
    loader2.load_data_into("BTCUSDT", CANDLES, &mut btc_close, &mut btc_high, &mut btc_low, &mut btc_open, &mut btc_vol, &mut btc_dt)?;

    let mut max_consec_loss = 0usize;
    let mut cur_consec = 0usize;
    let trade_dates: Vec<i64> = all_trades.iter().map(|t| t.entry_date).collect();

    for i in 1..btc_close.len() {
        let daily_ret = if btc_close[i-1] > 0.0 {
            (btc_close[i] - btc_close[i-1]) / btc_close[i-1]
        } else { 0.0 };
        if daily_ret < 0.0 {
            cur_consec += 1;
            max_consec_loss = max_consec_loss.max(cur_consec);
        } else {
            cur_consec = 0;
        }
    }

    // ── Distribution buckets ────────────────────────────────────────────
    let mut bucket_1 = 0usize; // < -5%
    let mut bucket_2 = 0usize; // -5% to -1%
    let mut bucket_3 = 0usize; // -1% to 0%
    let mut bucket_4 = 0usize; // 0% to 5%
    let mut bucket_5 = 0usize; // 5% to 20%
    let mut bucket_6 = 0usize; // > 20%

    for t in &all_trades {
        let p = t.net_pnl;
        if p < -0.05 { bucket_1 += 1; }
        else if p < -0.01 { bucket_2 += 1; }
        else if p < 0.0 { bucket_3 += 1; }
        else if p < 0.05 { bucket_4 += 1; }
        else if p < 0.20 { bucket_5 += 1; }
        else { bucket_6 += 1; }
    }

    println!("--- Trade Distribution ---");
    println!("Total trades:   {}", n);
    println!("Wins:          {} ({:.1}%)", wins, win_rate * 100.0);
    println!("Losses:        {} ({:.1}%)", losses, (1.0 - win_rate) * 100.0);
    println!("Avg win:       {:.2}%", avg_win * 100.0);
    println!("Avg loss:      {:.2}%", avg_loss * 100.0);
    println!("Avg W/L ratio: {:.2}x", avg_win_loss_ratio);
    println!();
    println!("--- PnL Buckets ---");
    println!("< -5%:         {:4} trades ({:5.1}%)", bucket_1, bucket_1 as f64 / n as f64 * 100.0);
    println!("-5% to -1%:   {:4} trades ({:5.1}%)", bucket_2, bucket_2 as f64 / n as f64 * 100.0);
    println!("-1% to 0%:    {:4} trades ({:5.1}%)", bucket_3, bucket_3 as f64 / n as f64 * 100.0);
    println!(" 0% to 5%:    {:4} trades ({:5.1}%)", bucket_4, bucket_4 as f64 / n as f64 * 100.0);
    println!(" 5% to 20%:   {:4} trades ({:5.1}%)", bucket_5, bucket_5 as f64 / n as f64 * 100.0);
    println!("> 20%:        {:4} trades ({:5.1}%)", bucket_6, bucket_6 as f64 / n as f64 * 100.0);
    println!();
    println!("--- Equity Decomposition ---");
    println!("Top-5 trades:  {:.1}% of total equity ({}/{} trades)", top5_pct, 5, n);
    println!("Top-10 trades: {:.1}% of total equity ({}/{} trades)", top10_pct, 10, n);
    println!("Top-20 trades: {:.1}% of total equity ({}/{} trades)", top20_pct, 20, n);
    println!();
    println!("--- Cost Analysis ---");
    println!("Gross PnL sum:  {:.4} (cumulative return mult)", gross_sum);
    println!("Total fees:     {:.4} ({:.1}% of gross)", fee_sum, fee_pct_of_gross);
    println!("Net PnL sum:    {:.4}", net_sum);
    println!();
    println!("--- Risk Metrics ---");
    println!("Max consecutive losing bars: {}", max_consec_loss);
    println!("Max drawdown (equity):  see T59 (99.3%)");
    println!();

    // ── Top-10 trades detail ────────────────────────────────────────────
    println!("--- Top-10 Trades by Net PnL ---");
    println!("{:>5} {:>12} {:>12} {:>10} {:>10} {:>8}", "Rank", "Entry", "Exit", "Gross%", "Fee%", "Net%");
    for (rank, &(i, pnl)) in with_pnl.iter().take(10).enumerate() {
        let t = &all_trades[i];
        println!(
            "{:>5} {:>12} {:>12} {:>10.2} {:>10.2} {:>8.2}",
            rank + 1,
            t.symbol,
            format!("{:.4}", t.net_pnl * 100.0),
            format!("{:.4}", t.gross_pnl * 100.0),
            format!("{:.4}", t.fee * 100.0),
            format!("{:.2}", t.bars_held),
        );
    }
    println!();

    // ── Bottom-10 trades ──────────────────────────────────────────────
    println!("--- Bottom-10 Trades by Net PnL (worst losers) ---");
    println!("{:>5} {:>12} {:>12} {:>10} {:>10} {:>8}", "Rank", "Entry", "Exit", "Gross%", "Fee%", "Net%");
    for (rank, &(i, _)) in with_pnl.iter().rev().take(10).enumerate() {
        let t = &all_trades[i];
        println!(
            "{:>5} {:>12} {:>12} {:>10.2} {:>10.2} {:>8.2}",
            rank + 1,
            t.symbol,
            format!("{:.4}", t.net_pnl * 100.0),
            format!("{:.4}", t.gross_pnl * 100.0),
            format!("{:.4}", t.fee * 100.0),
            format!("{:.2}", t.bars_held),
        );
    }

    // ── Write CSV ─────────────────────────────────────────────────────
    let mut csv = File::create("snapshots/turtle_pnl_attribution.csv")?;
    writeln!(csv, "rank,symbol,gross_pnl,fee,net_pnl,bars_held,entry_date,exit_date")?;
    for (rank, &(i, _)) in with_pnl.iter().enumerate() {
        let t = &all_trades[i];
        writeln!(csv, "{},{},{:.6},{:.6},{:.6},{},{},{}",
            rank + 1, t.symbol, t.gross_pnl, t.fee, t.net_pnl, t.bars_held, t.entry_date, t.exit_date)?;
    }

    let mut md = File::create("snapshots/turtle_pnl_attribution.md")?;
    writeln!(md, "# T63: Per-Bar PnL Attribution")?;
    writeln!(md, "\n## Summary")?;
    writeln!(md, "- Total trades: {}", n)?;
    writeln!(md, "- Win rate: {:.1}% ({}/{})", win_rate * 100.0, wins, losses)?;
    writeln!(md, "- Avg W/L ratio: {:.2}x", avg_win_loss_ratio)?;
    writeln!(md, "- Fee as % of gross: {:.1}%", fee_pct_of_gross)?;
    writeln!(md, "- Top-5 equity share: {:.1}%", top5_pct)?;
    writeln!(md, "- Top-10 equity share: {:.1}%", top10_pct)?;
    writeln!(md, "- Max consecutive losing bars: {}", max_consec_loss)?;
    writeln!(md, "\n## Buckets")?;
    writeln!(md, "| Bucket | Count | Pct |")?;
    writeln!(md, "|--------|-------|-----|")?;
    writeln!(md, "| < -5% | {} | {:.1}% |", bucket_1, bucket_1 as f64 / n as f64 * 100.0)?;
    writeln!(md, "| -5% to -1% | {} | {:.1}% |", bucket_2, bucket_2 as f64 / n as f64 * 100.0)?;
    writeln!(md, "| -1% to 0% | {} | {:.1}% |", bucket_3, bucket_3 as f64 / n as f64 * 100.0)?;
    writeln!(md, "| 0% to 5% | {} | {:.1}% |", bucket_4, bucket_4 as f64 / n as f64 * 100.0)?;
    writeln!(md, "| 5% to 20% | {} | {:.1}% |", bucket_5, bucket_5 as f64 / n as f64 * 100.0)?;
    writeln!(md, "| > 20% | {} | {:.1}% |", bucket_6, bucket_6 as f64 / n as f64 * 100.0)?;
    writeln!(md, "\n## Conclusion")?;
    if top5_pct > 60.0 {
        writeln!(md, "\n⚠️ FRAGILE: Top-5 trades account for {:.1}% of equity. Strategy is a bet on rare mega-trends.", top5_pct)?;
    } else if top5_pct > 40.0 {
        writeln!(md, "\n⚠️ MODERATE: Top-5 trades = {:.1}% of equity. Some concentration but distributed enough.", top5_pct)?;
    } else {
        writeln!(md, "\n✅ ROBUST: Top-5 trades = {:.1}% of equity. Edge is distributed.", top5_pct)?;
    }
    if fee_pct_of_gross > 20.0 {
        writeln!(md, "\n⚠️ Fee drag: {:.1}% of gross PnL consumed by fees.", fee_pct_of_gross)?;
    } else {
        writeln!(md, "\n✅ Fee drag acceptable: {:.1}% of gross.", fee_pct_of_gross)?;
    }

    println!("\nCSV: snapshots/turtle_pnl_attribution.csv");
    println!("Report: snapshots/turtle_pnl_attribution.md");

    Ok(())
}
