//! Donchian Entry Walk-Forward (T19)
//!
//! Donchian entry: close[idx] > max(high[start..idx-1])
//! Turtle entry:    close[idx] > max(close[start..idx-1])
//! Both use Turtle ATR(24, 2.0) + Chandelier(7, 2.30) dual exit.
//!
//! Run: Base5 (6 symbols) × 6 walk-forward windows.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 7;
const CHAND_MULT: f64 = 2.30;
const TURTLE_ENTRY: usize = 21;
const ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const VOL_LOOKBACK: usize = 9;
const ATR_ENTRY_MULT: f64 = 0.00;

const SYMBOLS: [&str; 6] = ["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"];

const CSV_OUT: &str = "snapshots/donchian_walkforward.csv";
const MD_OUT: &str = "snapshots/donchian_walkforward.md";

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
        let c0 = close.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    if trs.is_empty() { return 0.0; }
    trs.iter().sum::<f64>() / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return *vals.get(idx).unwrap_or(&0.0); }
    vals[idx + 1 - window..=idx].iter().sum::<f64>() / window as f64
}

// Donchian: close > max(high) over EP bars — strictest breakout (all-time high)
fn donchian_signal(high: &[f64], close: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_high = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&h) = high.get(i) { max_high = max_high.max(h); }
    }
    if let Some(&c) = close.get(idx) {
        return c > max_high;
    }
    false
}

// Turtle: close > max(close) over EP bars
fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    if let Some(&c) = close.get(idx) {
        return c > max_close;
    }
    false
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak { peak = e; }
        let dd = (peak - e) / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    use_donchian: bool,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Rank by dollar volume
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = sd.close.get(bar).copied().unwrap_or(0.0);
                let dv = rol_vol * price;
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect();

        if top_syms.is_empty() {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        // Entry signal (Donchian vs Turtle)
        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    let entry_signal = if use_donchian {
                        donchian_signal(&sd.high, &sd.close, TURTLE_ENTRY, bar)
                    } else {
                        turtle_signal(&sd.close, TURTLE_ENTRY, bar)
                    };

                    if entry_signal {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut lowest_low_turtle = sd.low[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;
                            lowest_low_turtle = lowest_low_turtle.min(sd.low[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, ATR_PERIOD, b);
                            let trail_turtle = lowest_low_turtle - TURTLE_ATR_MULT * atr_turtle;
                            if sd.close[b] < trail_chand || sd.close[b] < trail_turtle {
                                exit_bar = b;
                                break;
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let gross_ret = exit / entry - 1.0;
                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;

                            wins += if gross_ret > 0.0 { 1 } else { 0 };
                            total_trades += 1;
                            equity *= 1.0 + gross_ret;

                            let avg_daily = gross_ret / bars_held as f64;
                            for _ in 0..bars_held {
                                daily_rets.push(avg_daily);
                            }

                            if equity > peak { peak = equity; }
                            equity_curve.push(equity);
                            bar = exit_bar + 1;
                            entered = true;
                            break;
                        }
                    }
                }
            }
        }

        if !entered {
            equity_curve.push(equity);
            bar += 1;
        }
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== Donchian vs Turtle Entry Walk-Forward (T19) ====");
    eprintln!("Symbols: Base5 | Entry: Donchian(high) vs Turtle(close) | Exit: dual ATR\n");

    let loader = DataLoader::new(None, None);
    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    for &sym in &SYMBOLS {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => { raw_cache.insert(sym.to_string(), df); }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    let min_len = raw_cache.values().map(|df| df.height()).min().unwrap_or(0);
    let n = min_len.min(2800);

    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in SYMBOLS {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = df.height().min(n);
            macro_rules! col_vec {
                ($name:expr) => {{
                    let chunked = df.column($name)?.f64()?;
                    chunked.into_iter().filter_map(|x| x).take(n_min).collect::<Vec<_>>()
                }};
            }
            sym_data_map.insert(sym.to_string(), SymData {
                close: col_vec!("close"),
                high:  col_vec!("high"),
                low:   col_vec!("low"),
                vol:   col_vec!("volume"),
            });
        }
    }
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    // Walk-forward windows
    let total_bars = n;
    let n_windows = (total_bars.saturating_sub(TRAIN_BARS)) / TEST_BARS;

    let mut csv_lines = vec!["window,don_pass,don_sharpe,don_return,don_dd,don_trades,don_wr,tur_pass,tur_sharpe,tur_return,tur_dd,tur_trades".to_string()];
    let mut md_lines = Vec::new();
    md_lines.push("# Donchian Entry Walk-Forward (T19)\n\n".to_string());
    md_lines.push("| Window | Donchian Pass | Don Sharpe | Don Ret% | Don Trades | Turtle Pass | Tur Sharpe | Tur Ret% | Tur Trades | ΔSharpe | ΔRet |\n".to_string());
    md_lines.push("|--------|---------------|------------|----------|------------|-------------|------------|----------|------------|---------|-----|\n".to_string());

    let mut don_passes = 0usize;
    let mut tur_passes = 0usize;
    let mut don_sharpe_sum = 0.0;
    let mut tur_sharpe_sum = 0.0;
    let mut don_ret_sum = 0.0;
    let mut tur_ret_sum = 0.0;
    let mut don_trades_total = 0usize;
    let mut tur_trades_total = 0usize;

    let syms: Vec<String> = SYMBOLS.iter().map(|s| s.to_string()).collect();

    for wi in 0..n_windows {
        let train_end = TRAIN_BARS + wi * TEST_BARS;
        let train_start = train_end - TRAIN_BARS;
        let test_start = train_end;
        let test_end = (train_end + TEST_BARS).min(total_bars);

        let test_bars = test_end - test_start;
        if test_bars < 30 { continue; }

        let don = run_sim(&sym_data_map, &syms, test_start, test_end, true);
        let tur = run_sim(&sym_data_map, &syms, test_start, test_end, false);

        don_passes += if don.pass { 1 } else { 0 };
        tur_passes += if tur.pass { 1 } else { 0 };
        don_sharpe_sum += don.sharpe;
        tur_sharpe_sum += tur.sharpe;
        don_ret_sum += don.ret;
        tur_ret_sum += tur.ret;
        don_trades_total += don.trades;
        tur_trades_total += tur.trades;

        let delta_s = don.sharpe - tur.sharpe;
        let delta_r = don.ret - tur.ret;

        eprintln!("W{:02}: Don {:>3} / Sh {:+.3} / Ret {:+.1}% / {} tr  |  Tur {:>3} / Sh {:+.3} / Ret {:+.1}% / {} tr  | ΔSh {:+.3}",
            wi, if don.pass {"PASS"} else {"FAIL"}, don.sharpe, don.ret, don.trades,
            if tur.pass {"PASS"} else {"FAIL"}, tur.sharpe, tur.ret, tur.trades, delta_s);

        csv_lines.push(format!("{},{},{},{},{},{},{},{},{},{},{},{}",
            wi, don.pass, don.sharpe, don.ret, don.max_dd, don.trades, don.win_rate,
            tur.pass, tur.sharpe, tur.ret, tur.max_dd, tur.trades));

        md_lines.push(format!("| W{:02} | {} | {:+.3} | {:+.1}% | {} | {} | {:+.3} | {:+.1}% | {} | {:+.3} | {:+.1}% |\n",
            wi,
            if don.pass {"✅"} else {"❌"}, don.sharpe, don.ret, don.trades,
            if tur.pass {"✅"} else {"❌"}, tur.sharpe, tur.ret, tur.trades,
            delta_s, delta_r));
    }

    let n = n_windows as f64;
    eprintln!("\n=== SUMMARY ({} windows) ===", n_windows);
    eprintln!("Donchian: {}/{} pass | avg Sharpe {:+.3} | avg Return {:+.1}% | {} trades",
        don_passes, n_windows, don_sharpe_sum / n, don_ret_sum / n, don_trades_total);
    eprintln!("Turtle:   {}/{} pass | avg Sharpe {:+.3} | avg Return {:+.1}% | {} trades",
        tur_passes, n_windows, tur_sharpe_sum / n, tur_ret_sum / n, tur_trades_total);
    eprintln!("Elapsed: {:.1}s", t0.elapsed().as_secs_f64());

    // Write CSV
    let csv_content = csv_lines.join("\n");
    let mut f = File::create(CSV_OUT)?;
    f.write_all(csv_content.as_bytes())?;
    eprintln!("Saved: {}", CSV_OUT);

    // Write MD
    let mut md = String::new();
    md.push_str("# Donchian Entry Walk-Forward (T19)\n\n");
    md.push_str("**Entry difference:** Donchian = `close > max(high)` (strictest — all-time high breakout). ");
    md.push_str("Turtle = `close > max(close)` (breakout above highest close).\n\n");
    md.push_str("**Exit:** Both use Chandelier(7,2.30) + Turtle ATR(24,2.0) dual exit — identical.\n\n");
    md.push_str("| Metric | Donchian | Turtle | Delta |\n");
    md.push_str("|--------|----------|--------|-------|\n");
    md.push_str(&format!("| Pass Rate | {}/{} ({:.0}%) | {}/{} ({:.0}%) | {:+} pp |\n",
        don_passes, n_windows, 100.0*don_passes as f64/n,
        tur_passes, n_windows, 100.0*tur_passes as f64/n,
        (100.0*don_passes as f64/n - 100.0*tur_passes as f64/n) as i32));
    md.push_str(&format!("| Avg Sharpe | {:+.3} | {:+.3} | {:+.3} |\n",
        don_sharpe_sum/n, tur_sharpe_sum/n, (don_sharpe_sum - tur_sharpe_sum)/n));
    md.push_str(&format!("| Avg Return | {:+.1}% | {:+.1}% | {:+.1}% |\n",
        don_ret_sum/n, tur_ret_sum/n, (don_ret_sum - tur_ret_sum)/n));
    md.push_str(&format!("| Total Trades | {} | {} | {} |\n",
        don_trades_total, tur_trades_total, don_trades_total as i32 - tur_trades_total as i32));
    md.push('\n');

    if don_sharpe_sum > tur_sharpe_sum {
        md.push_str("**Result: Donchian WINS** — tighter entry produces higher quality signals.\n");
    } else if tur_sharpe_sum > don_sharpe_sum {
        md.push_str("**Result: Turtle WINS** — more permissive entry captures more edge despite lower signal quality.\n");
    } else {
        md.push_str("**Result: TIE** — entry method makes no meaningful difference.\n");
    }
    md.push('\n');
    md.push_str("## Per-Window Results\n\n");
    for line in &md_lines {
        md.push_str(line);
    }

    let mut f = File::create(MD_OUT)?;
    f.write_all(md.as_bytes())?;
    eprintln!("Saved: {}", MD_OUT);

    Ok(())
}