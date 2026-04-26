//! T13: CTREND Regime-Conditional Switching Walk-Forward
//!
//! Purpose: Test regime-conditional allocation instead of fixed sleeve.
//! - T6-NEXT FAILED: 75% Turtle + 25% CTREND fixed sleeve -> Sharpe 1.38->0.33 (-76%)
//! - Key insight: CTREND helps in choppy windows (+7-15pp DD) but costs too much in trending windows.
//! - T13 hypothesis: Switch to CTREND ONLY when regime is choppy. Trending -> pure Turtle.
//!
//! Regime classifier: ATR percentile rank (21-bar realized vs 252-bar history)
//!   - ATR rank > 70th pctile -> trending -> Turtle
//!   - ATR rank < 30th pctile -> ranging -> CTREND
//!
//! Win condition: Sharpe > 1.10 AND pass rate >= 75% (vs Turtle-only 1.38)

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const SYMBOLS: &[&str] = &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"];
const CANDLES: u32 = 3000;
const TAKER_FEE: f64 = 0.0004;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;

// Production Turtle params (frozen)
const TURTLE_ENTRY: usize = 21;
const CHAND_PERIOD: usize = 7;
const CHAND_MULT: f64 = 2.30;
const HOLD_MAX: usize = 12;
const TURTLE_ATR_PERIOD: usize = 24;

// CTREND params
const CTREND_FAST: usize = 8;
const CTREND_SLOW: usize = 32;
const CTREND_HOLD: usize = 30;

// Regime classifier thresholds
const VOL_PCTILE_HIGH: f64 = 0.70; // top 30% -> trending -> Turtle
const VOL_PCTILE_LOW: f64 = 0.30;  // bottom 30% -> ranging -> CTREND

// ─────────────────────────────────────────────────────────────────────────────

fn ema(data: &[f64], period: usize) -> Vec<f64> {
    let a = 2.0 / (period as f64 + 1.0);
    let n = data.len();
    let mut out = vec![0.0; n];
    for i in 0..n {
        out[i] = if i == 0 { data[0] } else { a * data[i] + (1.0 - a) * out[i - 1] };
    }
    out
}

fn atr(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut sum = 0.0;
    for i in (idx + 1 - period)..=idx {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c = close.get(i.saturating_sub(1)).copied().unwrap_or(close[0]);
        let tr = (h - l).max((h - c).abs()).max((l - c).abs());
        sum += tr;
    }
    sum / period as f64
}

fn max_close(close: &[f64], lookback: usize, idx: usize) -> f64 {
    if idx < lookback { return 0.0; }
    close[idx + 1 - lookback..idx].iter().fold(0.0_f64, |m, &v| m.max(v))
}

fn truerange(high: &[f64], low: &[f64], close: &[f64], idx: usize) -> f64 {
    if idx == 0 { return high[0] - low[0]; }
    let h = high[idx];
    let l = low[idx];
    let pc = close[idx - 1];
    (h - l).max((h - pc).abs()).max((l - pc).abs())
}

fn atr_percentile_rank(atr_val: f64, atr_history: &[f64]) -> f64 {
    if atr_history.is_empty() { return 0.5; }
    let above = atr_history.iter().filter(|&&x| x < atr_val).count() as f64;
    above / atr_history.len() as f64
}

// ─────────────────────────────────────────────────────────────────────────────

struct EqPool {
    equity: Vec<f64>,
    trades: Vec<f64>,
}

impl EqPool {
    fn new() -> Self { EqPool { equity: vec![1.0], trades: Vec::new() } }
    fn record_trade(&mut self, ret: f64) {
        let last = *self.equity.last().unwrap();
        self.equity.push(last * (1.0 + ret));
        self.trades.push(ret);
    }
    fn finalize(&self) -> (f64, f64, f64, usize) {
        let eq = *self.equity.last().unwrap();
        let peak = self.equity.iter().fold(0.0_f64, |m, &v| m.max(v));
        let trough = self.equity.iter().fold(1.0_f64, |m, &v| m.min(v));
        let dd = if peak > 0.0 { (peak - trough) / peak } else { 0.0 };
        let daily_returns: Vec<f64> = self.equity.windows(2).map(|w| (w[1] - w[0]) / w[0]).collect();
        let (mean, std) = if daily_returns.len() > 1 {
            let sum: f64 = daily_returns.iter().sum();
            let m = sum / daily_returns.len() as f64;
            let var: f64 = daily_returns.iter().map(|r| { let d = r - m; d * d }).sum::<f64>() / daily_returns.len() as f64;
            (m, var.sqrt())
        } else { (0.0, 1.0) };
        let sharpe = if std > 0.0 { (mean / std) * (252.0_f64.sqrt()) } else { 0.0 };
        (eq, dd * 100.0, sharpe, self.trades.len())
    }
}

// ─────────────────────────────────────────────────────────────────────────────

fn run_turtle(close: &[f64], high: &[f64], low: &[f64], start_idx: usize, n_bars: usize) -> (f64, f64, f64, usize) {
    let mut pool = EqPool::new();
    for bar in 0..n_bars {
        let idx = start_idx + bar;
        let open_next = *close.get(idx + 1).unwrap_or(&close[idx]);
        let max_h = max_close(close, TURTLE_ENTRY, idx);
        if close[idx] > max_h && max_h > 0.0 {
            let mut exit_price = open_next;
            for k in 0..HOLD_MAX.min(n_bars - bar) {
                let t_idx = idx + 1 + k;
                if t_idx >= close.len() { break; }
                let atr_v = atr(high, low, close, CHAND_PERIOD, t_idx);
                let long_exit = close.get(t_idx).map(|&c| c < close[idx] - CHAND_MULT * atr_v).unwrap_or(false)
                    || close.get(t_idx).map(|&c| c > close[idx] + CHAND_MULT * atr_v).unwrap_or(false);
                let tatr = atr(high, low, close, TURTLE_ATR_PERIOD, t_idx);
                let turtle_exit = close.get(t_idx).map(|&c| c < open_next - 2.0 * tatr).unwrap_or(false);
                if long_exit || turtle_exit || k == HOLD_MAX - 1 {
                    exit_price = *close.get(t_idx).unwrap_or(&open_next);
                    break;
                }
            }
            let ret = (exit_price - open_next) / open_next - 2.0 * TAKER_FEE;
            pool.record_trade(ret);
        }
    }
    pool.finalize()
}

fn run_ctrend(close: &[f64], start_idx: usize, n_bars: usize) -> (f64, f64, f64, usize) {
    let mut pool = EqPool::new();
    for bar in 0..n_bars {
        let idx = start_idx + bar;
        let open_next = *close.get(idx + 1).unwrap_or(&close[idx]);
        let fast_start = if bar >= CTREND_FAST { idx + 1 - CTREND_FAST } else { start_idx };
        let slow_start = if bar >= CTREND_SLOW { idx + 1 - CTREND_SLOW } else { start_idx };
        let end_idx = idx.min(close.len() - 1);
        let fast_ema = ema(&close[fast_start..=end_idx], CTREND_FAST);
        let slow_ema = ema(&close[slow_start..=end_idx], CTREND_SLOW);
        if !fast_ema.is_empty() && !slow_ema.is_empty()
            && *fast_ema.last().unwrap() > *slow_ema.last().unwrap()
        {
            let hold_bars = CTREND_HOLD.min(n_bars - bar);
            let exit_idx = (idx + 1 + hold_bars).min(close.len() - 1);
            let exit_p = *close.get(exit_idx).unwrap_or(&open_next);
            let ret = (exit_p - open_next) / open_next - 2.0 * TAKER_FEE;
            pool.record_trade(ret);
        }
    }
    pool.finalize()
}

fn run_conditional(close: &[f64], high: &[f64], low: &[f64], start_idx: usize, n_bars: usize) -> (f64, f64, f64, usize) {
    let atr_history: Vec<f64> = (0..start_idx).map(|i| truerange(high, low, close, i)).collect();
    let mut pool = EqPool::new();
    for bar in 0..n_bars {
        let idx = start_idx + bar;
        let open_next = *close.get(idx + 1).unwrap_or(&close[idx]);
        let atr_current = truerange(high, low, close, idx);
        let regime_pct = atr_percentile_rank(atr_current, &atr_history);
        let max_h = max_close(close, TURTLE_ENTRY, idx);
        let turtle_sig = close[idx] > max_h && max_h > 0.0;
        let end_idx = idx.min(close.len() - 1);
        let fast_start = if bar >= CTREND_FAST { idx + 1 - CTREND_FAST } else { start_idx };
        let slow_start = if bar >= CTREND_SLOW { idx + 1 - CTREND_SLOW } else { start_idx };
        let fast_ema = ema(&close[fast_start..=end_idx], CTREND_FAST);
        let slow_ema = ema(&close[slow_start..=end_idx], CTREND_SLOW);
        let ctrend_sig = !fast_ema.is_empty() && !slow_ema.is_empty()
            && *fast_ema.last().unwrap() > *slow_ema.last().unwrap();

        if turtle_sig {
            let mut exit_price = open_next;
            for k in 0..HOLD_MAX.min(n_bars - bar) {
                let t_idx = idx + 1 + k;
                if t_idx >= close.len() { break; }
                let atr_v = atr(high, low, close, CHAND_PERIOD, t_idx);
                let long_exit = close.get(t_idx).map(|&c| c < close[idx] - CHAND_MULT * atr_v).unwrap_or(false)
                    || close.get(t_idx).map(|&c| c > close[idx] + CHAND_MULT * atr_v).unwrap_or(false);
                let tatr = atr(high, low, close, TURTLE_ATR_PERIOD, t_idx);
                let turtle_exit = close.get(t_idx).map(|&c| c < open_next - 2.0 * tatr).unwrap_or(false);
                if long_exit || turtle_exit || k == HOLD_MAX - 1 {
                    exit_price = *close.get(t_idx).unwrap_or(&open_next);
                    break;
                }
            }
            let ret = (exit_price - open_next) / open_next - 2.0 * TAKER_FEE;
            pool.record_trade(ret);
        } else if ctrend_sig && regime_pct < VOL_PCTILE_LOW {
            let hold_bars = CTREND_HOLD.min(n_bars - bar);
            let exit_idx = (idx + 1 + hold_bars).min(close.len() - 1);
            let exit_p = *close.get(exit_idx).unwrap_or(&open_next);
            let ret = (exit_p - open_next) / open_next - 2.0 * TAKER_FEE;
            pool.record_trade(ret);
        }
    }
    pool.finalize()
}

// ─────────────────────────────────────────────────────────────────────────────

// Format helpers to avoid `.Nf` format issues in Rust 1.95
fn f3(v: f64) -> String { format!("{:.3}", v) }
fn f2(v: f64) -> String { format!("{:.2}", v) }
fn f4(v: f64) -> String { format!("{:.4}", v) }

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    println!("T13: CTREND Regime-Conditional Switching Walk-Forward");
    println!("Regime: 21-bar ATR pct rank vs 252-bar history");
    println!("High ATR (>70th pct) -> Turtle | Low ATR (<30th pct) -> CTREND");

    let loader = DataLoader::new(None, None);
    let mut cache: HashMap<String, DataFrame> = HashMap::new();
    for &sym in SYMBOLS {
        if let Ok(df) = loader.fetch_with_cache(sym, "1d", CANDLES).await {
            cache.insert(sym.to_string(), df);
        }
    }
    eprintln!("Loaded {} symbols", cache.len());
    let n_min = cache.values().map(|df| df.height()).min().unwrap_or(0);
    let n = n_min.min(2800);

    macro_rules! cv {
        ($df:expr, $name:expr) => {{
            let c = $df.column($name)?.f64()?;
            c.into_iter().filter_map(|x| x).take(n).collect::<Vec<f64>>()
        }};
    }

    let mut data: HashMap<String, (Vec<f64>, Vec<f64>, Vec<f64>)> = HashMap::new();
    for sym in SYMBOLS {
        if let Some(df) = cache.get(&sym.to_string()) {
            data.insert(sym.to_string(), (cv!(df, "close"), cv!(df, "high"), cv!(df, "low")));
        }
    }

    // Total windows based on TRAIN_BARS + TEST_BARS structure
    let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
    eprintln!("Total walk-forward windows: {}", total_windows);

    let mut csv_file = File::create("snapshots/t13_regime_conditional_results.csv")?;
    writeln!(csv_file, "symbol,window,turtle_eq,turtle_dd,turtle_sharpe,turtle_n,ctrend_eq,ctrend_dd,ctrend_sharpe,ctrend_n,cond_eq,cond_dd,cond_sharpe,cond_n")?;

    let mut turtle_sharpes = Vec::new();
    let mut ctrend_sharpes = Vec::new();
    let mut cond_sharpes = Vec::new();

    for sym in SYMBOLS {
        if let Some((close, high, low)) = data.get(&sym.to_string()) {
            print!("  {} ", sym);
            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { continue; }
                let nb = test_end - test_start;

                let (t_eq, t_dd, t_sh, t_n) = run_turtle(close, high, low, test_start, nb);
                let (c_eq, c_dd, c_sh, c_n) = run_ctrend(close, test_start, nb);
                let (cond_eq, cond_dd, cond_sh, cond_n) = run_conditional(close, high, low, test_start, nb);

                println!("W{:02}/T={}/C={}/X={}", wi, f3(t_eq), f3(c_eq), f3(cond_eq));
                writeln!(csv_file, "{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                    sym, wi, f4(t_eq), f2(t_dd), f4(t_sh), t_n,
                    f4(c_eq), f2(c_dd), f4(c_sh), c_n,
                    f4(cond_eq), f2(cond_dd), f4(cond_sh), cond_n)?;

                if t_n >= 3 { turtle_sharpes.push(t_sh); }
                if c_n >= 3 { ctrend_sharpes.push(c_sh); }
                if cond_n >= 3 { cond_sharpes.push(cond_sh); }
            }
            println!("  done");
        }
    }

    let avg_t = if !turtle_sharpes.is_empty() { turtle_sharpes.iter().sum::<f64>() / turtle_sharpes.len() as f64 } else { 0.0 };
    let avg_c = if !ctrend_sharpes.is_empty() { ctrend_sharpes.iter().sum::<f64>() / ctrend_sharpes.len() as f64 } else { 0.0 };
    let avg_x = if !cond_sharpes.is_empty() { cond_sharpes.iter().sum::<f64>() / cond_sharpes.len() as f64 } else { 0.0 };
    let pass_t = turtle_sharpes.iter().filter(|&&s| s > 0.0).count();
    let pass_c = ctrend_sharpes.iter().filter(|&&s| s > 0.0).count();
    let pass_x = cond_sharpes.iter().filter(|&&s| s > 0.0).count();
    let total = turtle_sharpes.len();

    println!("\n=== RESULTS ===");
    println!("Turtle-only:  {}/{} pass, avg Sharpe {}", pass_t, total, f3(avg_t));
    println!("CTREND-only:  {}/{} pass, avg Sharpe {}", pass_c, total, f3(avg_c));
    println!("Conditional:  {}/{} pass, avg Sharpe {}", pass_x, total, f3(avg_x));

    let target = 1.10_f64;
    if avg_x >= target && pass_x >= total * 3 / 4 {
        println!("\nPASS -- Conditional Sharpe {} >= {:.3} target, {} >= 75% pass", f3(avg_x), target, pass_x);
    } else {
        println!("\nFAIL -- Conditional Sharpe {} < {:.3} target OR pass < 75%", f3(avg_x), target);
    }
    println!("Time: {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}