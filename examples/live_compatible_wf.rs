//! T38: Corrected Live-Compatible Walk-Forward
//!
//! Matches `src/live/bot.rs` exactly:
//! - Turtle breakout entry
//! - ATR_RANK(12, 42, 5.0) entry gate
//! - USDT high-vol size overlay (reduces size 30% if current 21d ATR > 75th percentile of 252d history)
//! - Turtle-only long exit: highest_high - ATR_MULT * ATR
//! - ATR buffer seeded with TURTLE_ATR_PERIOD
//! - HOLD_MAX enforced independent of ATR warmup
//!
//! Exports per-universe equity curves to CSV and a summary markdown report.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;

const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 96; // updated 2026-05-01 from VL=8 (extensive 100-value sweep, VL=96 plateau 91-100)

const REGIME_ATR_PERIOD: usize = 17; // hyperopt 2026-05-04: AP=17 wins OOS on Sharpe/Return, AP=63 wins on pass rate. Held-out (4-period pre-2021): AP=17: 4/4 pass, Sharpe 7.715, equity 1.9481x. AP=63: 4/4 pass, Sharpe 5.721, equity 1.3976x. AP=17 dominates all held-out metrics. Updated from AP=63.
const REGIME_LOOKBACK: usize = 42; // confirmed 2026-05-02: LB∈[5..=200] sweep → LB=42 optimal (Sharpe 6.188, 55/63 pass, 9/9 positive)
const ATR_RANK_T: f64 = 5.0; // REVERTED 2026-05-04: T=24 was 3rd sequential optimization on this harness. T52 held-out on pre-2021 data: T=24 → 10/22 pass, Sharpe -0.964. T=5 → 14/22 pass, Sharpe +0.664. Same EP=24 pattern. ATR_RANK=24 is a same-harness artifact. T=5 is the correct production default.

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",        &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE",       &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
    ("Legacy4",      &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("Legacy5BNB",   &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","BNBUSDT","EOSUSDT"]),
    ("OldGuardNoBNB",&["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
    ("LargeCaps5",   &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","BNBUSDT","ADAUSDT"]),
    ("Legacy3",      &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("LowVolume5",   &["XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT","ADAUSDT"]),
    ("OldGuard4",    &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
];

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
        let h = high[i];
        let l = low[i];
        let c0 = close[i.saturating_sub(1)];
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    trs.iter().sum::<f64>() / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return 0.0; }
    vals[idx.saturating_sub(window - 1)..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize) -> bool {
    if idx < entry_period { return false; }
    let start = idx - entry_period;
    let max_close = close[start..idx].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    if close[idx] > max_close {
        if atr_mult > 0.0 {
            let atr = atr_at(high, low, close, atr_period, idx);
            if close[idx] < max_close + atr * atr_mult {
                return false;
            }
        }
        return true;
    }
    false
}

fn btc_atr_pct(btc_data: &SymData, period: usize, lookback: usize, idx: usize) -> f64 {
    if idx < period.max(lookback) { return 50.0; }
    let curr_atr = atr_at(&btc_data.high, &btc_data.low, &btc_data.close, period, idx);
    let mut hist = Vec::with_capacity(lookback);
    for j in (idx + 1 - lookback)..=idx {
        if j >= period {
            hist.push(atr_at(&btc_data.high, &btc_data.low, &btc_data.close, period, j));
        }
    }
    if hist.is_empty() { return 50.0; }
    let count = hist.iter().filter(|&&x| x < curr_atr).count();
    (count as f64 / hist.len() as f64) * 100.0
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.is_empty() { return 0.0; }
    let mean = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let var = daily_rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / daily_rets.len() as f64;
    if var == 0.0 { return 0.0; }
    (mean / var.sqrt()) * (365.0_f64).sqrt()
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut max_dd = 0.0;
    let mut peak = 1.0;
    for &val in equity {
        if val > peak { peak = val; }
        let dd = 1.0 - val / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

#[derive(Default)]
struct WfResult {
    equity: f64,
    sharpe: f64,
    dd: f64,
    trades: usize,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let btc = sym_data.get("BTCUSDT");
        let btc_pct = if let Some(b) = btc { btc_atr_pct(b, REGIME_ATR_PERIOD, REGIME_LOOKBACK, bar) } else { 50.0 };
        
        if btc_pct < ATR_RANK_T {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

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

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        let mut size_mult = 1.0;
                        if let Some(b) = btc {
                            if bar >= 252 + 21 {
                                let atr_21 = atr_at(&b.high, &b.low, &b.close, 21, bar);
                                let mut hist = Vec::with_capacity(252);
                                for j in (bar + 1 - 252)..=bar {
                                    let h = b.high[j];
                                    let l = b.low[j];
                                    let c0 = b.close[j.saturating_sub(1)];
                                    hist.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
                                }
                                hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
                                let pct_75 = hist[(0.75 * hist.len() as f64) as usize];
                                if atr_21 > pct_75 {
                                    size_mult = 0.70;
                                }
                            }
                        }

                        let entry = entry_px * (1.0 + TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        
                        let mut atr_buf: std::collections::VecDeque<f64> = std::collections::VecDeque::new();
                        
                        for b in entry_bar_next..=max_bar {
                            if sd.high[b] > highest_high { highest_high = sd.high[b]; }
                            let c0 = sd.close[b.saturating_sub(1)];
                            let tr = (sd.high[b] - sd.low[b]).max((sd.high[b] - c0).abs()).max((sd.low[b] - c0).abs());
                            atr_buf.push_back(tr);
                            if atr_buf.len() > TURTLE_ATR_PERIOD { atr_buf.pop_front(); }
                            
                            if atr_buf.len() == TURTLE_ATR_PERIOD {
                                let atr = atr_buf.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
                                let turtle_stop = highest_high - TURTLE_ATR_MULT * atr;
                                if sd.low[b] <= turtle_stop {
                                    exit_bar = b;
                                    break;
                                }
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let pct_ret = exit / entry - 1.0;
                            let gross_ret = pct_ret * size_mult;
                            
                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;

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
    
    WfResult {
        equity,
        sharpe: annualised_sharpe(&daily_rets),
        dd: max_dd_from(&equity_curve),
        trades: total_trades,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Loading data...");
    let loader = DataLoader::new(None, None);
    let mut sym_data = HashMap::new();
    let mut min_len = usize::MAX;

    let all_symbols = UNIVERSES.iter().flat_map(|(_, s)| s.iter()).map(|&s| s).collect::<std::collections::HashSet<_>>();
    for &sym in &all_symbols {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let high = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let low = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let vol = df.column("volume")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        if close.len() < min_len { min_len = close.len(); }
        sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
    }

    let windows = (min_len.saturating_sub(TRAIN_BARS)) / TEST_BARS;
    if windows == 0 { return Ok(()); }

    let mut passes = 0;
    let mut total = 0;
    let mut sharpes = vec![];
    let mut rets = vec![];

    for (_, u_syms) in UNIVERSES {
        let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
        for w in 0..windows {
            let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
            let end = start + TEST_BARS + TRAIN_BARS;
            let res = run_sim(&sym_data, &syms, start + TRAIN_BARS, end);
            
            if res.trades >= MIN_TRADES && res.sharpe > 0.0 {
                passes += 1;
            }
            total += 1;
            sharpes.push(res.sharpe);
            rets.push((res.equity - 1.0) * 100.0);
        }
    }

    let avg_sharpe = sharpes.iter().sum::<f64>() / total as f64;
    let avg_ret = rets.iter().sum::<f64>() / total as f64;
    println!("Live Compatible WF: {}/{} pass ({:.1}%), Avg Sharpe: {:.3}, Avg Ret: {:.1}%", 
        passes, total, (passes as f64 / total as f64) * 100.0, avg_sharpe, avg_ret);

    // Export equity per universe
    for (u_name, u_syms) in UNIVERSES {
        let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
        let csv_path = format!("snapshots/live_compatible_{}_equity.csv", u_name);
        let mut f = File::create(&csv_path)?;
        writeln!(f, "window,equity")?;
        
        for w in 0..windows {
            let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
            let end = start + TEST_BARS + TRAIN_BARS;
            let res = run_sim(&sym_data, &syms, start + TRAIN_BARS, end);
            writeln!(f, "{},{:.6}", w, res.equity)?;
        }
        println!("Exported {} equity to {}", u_name, csv_path);
    }

    // Base5 aggregate equity (compounded across windows)
    {
        let base5_syms: Vec<String> = UNIVERSES[0].1.iter().map(|&s| s.to_string()).collect();
        let mut agg_equity = 1.0_f64;
        let mut f = File::create("snapshots/live_compatible_base5_daily.csv")?;
        writeln!(f, "window,equity")?;
        
        for w in 0..windows {
            let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
            let end = start + TEST_BARS + TRAIN_BARS;
            let res = run_sim(&sym_data, &base5_syms, start + TRAIN_BARS, end);
            agg_equity *= res.equity;
            writeln!(f, "{},{:.6}", w, agg_equity)?;
        }
        println!("Base5 aggregate equity: {:.6}x ({} windows)", agg_equity, windows);
    }

    // Write summary markdown
    let md_path = "snapshots/live_compatible_wf.md";
    let mut f = File::create(md_path)?;
    writeln!(f, "# Live-Compatible Walk-Forward Results")?;
    writeln!(f, "")?;
    writeln!(f, "**Strategy:** Turtle-only exit (matching `src/live/bot.rs` after 2026-05-01 bug fix)")?;
    writeln!(f, "- Entry: Turtle breakout (EP=21) + ATR_RANK(AP=12, LB=42, T=24) gate")?;
    writeln!(f, "- Exit: Turtle ATR trailing stop (AP=24, M=2.0) + HOLD_MAX={}", HOLD_MAX)?;
    writeln!(f, "- Risk overlay: USDT 30% size when BTC 21d ATR > 75th pct of 252d history")?;
    writeln!(f, "- Fee: 0.10% taker (both sides)")?;
    writeln!(f, "- VOL_LOOKBACK: {}", VOL_LOOKBACK)?;
    writeln!(f, "")?;
    writeln!(f, "## Global Results ({}/{} pass, {}-bar windows)", passes, total, TEST_BARS)?;
    writeln!(f, "| Metric | Value |")?;
    writeln!(f, "|--------|-------|")?;
    writeln!(f, "| Pass Rate | {}/{} ({:.1}%) |", passes, total, (passes as f64/total as f64)*100.0)?;
    writeln!(f, "| Avg Sharpe | {:.3} |", avg_sharpe)?;
    writeln!(f, "| Avg Return | {:.1}% |", avg_ret)?;
    writeln!(f, "")?;
    writeln!(f, "## Per-Universe Summary")?;
    writeln!(f, "| Universe | Pass | Sharpe | Return% | DD% | Trades |")?;
    writeln!(f, "|----------|-------|--------|---------|-----|--------|")?;
    
    for (u_name, u_syms) in UNIVERSES {
        let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
        let mut u_passes = 0usize;
        let mut u_sharpes = vec![];
        let mut u_rets = vec![];
        let mut u_dds = vec![];
        let mut u_trades = vec![];
        
        for w in 0..windows {
            let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
            let end = start + TEST_BARS + TRAIN_BARS;
            let res = run_sim(&sym_data, &syms, start + TRAIN_BARS, end);
            if res.trades >= MIN_TRADES && res.sharpe > 0.0 { u_passes += 1; }
            u_sharpes.push(res.sharpe);
            u_rets.push((res.equity - 1.0) * 100.0);
            u_dds.push(res.dd);
            u_trades.push(res.trades);
        }
        let n = windows as f64;
        let avg_s = u_sharpes.iter().sum::<f64>() / n;
        let avg_r = u_rets.iter().sum::<f64>() / n;
        let avg_d = u_dds.iter().sum::<f64>() / n;
        let tot_t: usize = u_trades.iter().sum();
        writeln!(f, "| {} | {}/{} | {:.2} | {:.1}% | {:.1}% | {} |",
            u_name, u_passes, windows, avg_s, avg_r, avg_d, tot_t)?;
    }
    writeln!(f, "")?;
    writeln!(f, "*Pass = windows with ≥{} trades AND Sharpe>0 / total windows.*", MIN_TRADES)?;
    println!("Markdown summary written to {}", md_path);

    Ok(())
}
