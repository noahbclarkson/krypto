//! T80: Out-of-sample universe validation for the exact live Turtle path.
//!
//! Hold-out symbols: UNIUSDT, MATICUSDT, AVAXUSDT.
//! These are intentionally outside the repeatedly optimized 9-universe grid.
//!
//! Method: replay the current live-bot daily decision semantics on each symbol
//! independently for the most recent 6 walk-forward windows (252 train + 252
//! test bars). BTCUSDT is used only for the live ATR_RANK and hedge gates.

use anyhow::{Context, Result};
use chrono::Utc;
use krypto::data::loader::DataLoader;
use krypto::live::config::{
    LiveConfig, ATR_ENTRY_MULT, ATR_RANK_THRESHOLD, HEDGE_ATR_PCT, HEDGE_ATR_PERIOD,
    HEDGE_LOOKBACK, HEDGE_SIZE_MULT, HOLD_MAX, POSITION_CAP, REGIME_ATR_PERIOD, REGIME_LOOKBACK,
    TURTLE_ATR_MULT, TURTLE_ATR_PERIOD, TURTLE_EP, VOL_LOOKBACK,
};
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const WINDOWS: usize = 6;
const MIN_TRADES: usize = 3;
const HOLDOUT_SYMBOLS: [&str; 3] = ["UNIUSDT", "MATICUSDT", "AVAXUSDT"];

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    dates: Vec<String>,
}

#[derive(Clone)]
struct PositionState {
    entry_bar: usize,
    entry_exec: f64,
    size: f64,
    highest_high: f64,
    bars_held: usize,
    atr_buf: VecDeque<f64>,
}

#[derive(Default, Clone)]
struct WindowResult {
    symbol: String,
    window: usize,
    train_start: String,
    test_start: String,
    test_end: String,
    final_equity: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    entry_candidates: usize,
    atr_gate_skips: usize,
    hedged_entries: usize,
    pass: bool,
}

fn tr_at(sd: &SymData, idx: usize) -> f64 {
    let pc = if idx == 0 {
        sd.close[idx]
    } else {
        sd.close[idx - 1]
    };
    (sd.high[idx] - sd.low[idx])
        .max((sd.high[idx] - pc).abs())
        .max((sd.low[idx] - pc).abs())
}

fn atr_at(sd: &SymData, period: usize, idx: usize) -> f64 {
    if period == 0 || idx < period || idx >= sd.close.len() {
        return 0.0;
    }
    let start = idx + 1 - period;
    let mut sum = 0.0;
    for i in start..=idx {
        sum += tr_at(sd, i);
    }
    sum / period as f64
}

fn btc_atr_percentile(btc: &SymData, atr_period: usize, lookback: usize, idx: usize) -> f64 {
    let len = idx + 1;
    if len <= atr_period.max(lookback) + 1 {
        return 50.0;
    }

    let curr_atr = atr_at(btc, atr_period, idx);
    let curr_close = btc.close[idx];
    if curr_atr <= 0.0 || curr_close <= 0.0 {
        return 50.0;
    }
    let curr_pct = curr_atr / curr_close;

    let start = idx.saturating_sub(lookback);
    let mut below = 0usize;
    let mut total = 0usize;
    for i in start..idx {
        let close = btc.close[i];
        if close <= 0.0 {
            continue;
        }
        let hist_atr = atr_at(btc, atr_period, i);
        if hist_atr <= 0.0 {
            continue;
        }
        if hist_atr / close < curr_pct {
            below += 1;
        }
        total += 1;
    }

    if total == 0 {
        50.0
    } else {
        (below as f64 / total as f64) * 100.0
    }
}

fn hedge_active(btc: &SymData, idx: usize) -> bool {
    let n = idx + 1;
    if n < HEDGE_LOOKBACK + HEDGE_ATR_PERIOD {
        return false;
    }

    let mut trs = Vec::with_capacity(HEDGE_ATR_PERIOD);
    for i in (n - HEDGE_ATR_PERIOD)..n {
        trs.push(tr_at(btc, i));
    }
    let hedge_atr = trs.iter().sum::<f64>() / HEDGE_ATR_PERIOD as f64;

    let mut hist = Vec::with_capacity(HEDGE_LOOKBACK);
    for j in 1..=HEDGE_LOOKBACK {
        let hist_idx = n.saturating_sub(j);
        if hist_idx == 0 {
            break;
        }
        hist.push(tr_at(btc, hist_idx));
    }
    hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct_idx = (HEDGE_ATR_PCT * hist.len() as f64) as usize;
    hist.get(pct_idx)
        .is_some_and(|&threshold| hedge_atr > threshold)
}

/// Mirrors check_turtle_entry() in bot.rs: current-inclusive max close window,
/// equality allowed (`close < max_close` rejects; equality passes).
fn live_bot_entry_signal(sd: &SymData, idx: usize) -> bool {
    let len = idx + 1;
    if len < TURTLE_EP + 1 {
        return false;
    }
    let ws = len - TURTLE_EP;
    let max_close = sd.close[ws..=idx]
        .iter()
        .fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    if sd.close[idx] < max_close {
        return false;
    }

    if ATR_ENTRY_MULT > 0.0 && len >= TURTLE_ATR_PERIOD + 1 {
        let atr = atr_at(sd, TURTLE_ATR_PERIOD, idx);
        let threshold = max_close + atr * ATR_ENTRY_MULT;
        if sd.close[idx] < threshold {
            return false;
        }
    }
    true
}

fn seed_atr_buf(sd: &SymData, idx: usize) -> VecDeque<f64> {
    let len = idx + 1;
    let avail = len.min(TURTLE_ATR_PERIOD);
    let start = len.saturating_sub(avail);
    let mut atr_buf = VecDeque::with_capacity(TURTLE_ATR_PERIOD);
    for offset in 0..avail {
        let b_idx = start + offset;
        if b_idx >= len {
            break;
        }
        let pc = if offset == 0 {
            sd.close[b_idx]
        } else {
            sd.close[start + offset - 1]
        };
        let tr = (sd.high[b_idx] - sd.low[b_idx])
            .max((sd.high[b_idx] - pc).abs())
            .max((sd.low[b_idx] - pc).abs());
        atr_buf.push_back(tr);
    }
    atr_buf
}

fn annualised_sharpe_from_equity(equity: &[f64]) -> f64 {
    if equity.len() < 2 {
        return 0.0;
    }
    let rets: Vec<f64> = equity
        .windows(2)
        .filter_map(|w| {
            if w[0] > 0.0 {
                Some(w[1] / w[0] - 1.0)
            } else {
                None
            }
        })
        .collect();
    if rets.is_empty() {
        return 0.0;
    }
    let mean = rets.iter().sum::<f64>() / rets.len() as f64;
    let var = rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / rets.len() as f64;
    if var <= 0.0 {
        0.0
    } else {
        (mean / var.sqrt()) * 365.0_f64.sqrt()
    }
}

fn max_dd(equity: &[f64]) -> f64 {
    let mut peak = 1.0;
    let mut max_dd = 0.0;
    for &eq in equity {
        if eq > peak {
            peak = eq;
        }
        if peak > 0.0 {
            let dd = 1.0 - eq / peak;
            if dd > max_dd {
                max_dd = dd;
            }
        }
    }
    max_dd * 100.0
}

fn mark_to_market_equity(
    realized_equity: f64,
    pos: &Option<PositionState>,
    sd: &SymData,
    idx: usize,
    fee: f64,
) -> f64 {
    if let Some(pos) = pos {
        if idx < sd.close.len() {
            let liquidation_exec = sd.close[idx] * (1.0 - fee);
            return realized_equity * (1.0 + pos.size * (liquidation_exec / pos.entry_exec - 1.0));
        }
    }
    realized_equity
}

fn run_window(
    symbol: &str,
    sd: &SymData,
    btc: &SymData,
    test_start: usize,
    test_end: usize,
    window: usize,
    fee: f64,
) -> WindowResult {
    let mut realized_equity = 1.0_f64;
    let mut equity_curve: Vec<f64> = Vec::with_capacity(test_end.saturating_sub(test_start));
    let mut pos: Option<PositionState> = None;
    let mut trades = 0usize;
    let mut wins = 0usize;
    let mut entry_candidates = 0usize;
    let mut atr_gate_skips = 0usize;
    let mut hedged_entries = 0usize;

    for idx in test_start..test_end {
        if let Some(mut p) = pos.take() {
            if idx > p.entry_bar {
                if sd.high[idx] > p.highest_high {
                    p.highest_high = sd.high[idx];
                }
                p.bars_held += 1;

                // Mirrors bot.rs quirk as captured by T65: current close as previous close.
                let prev_close = sd.close[idx];
                let tr = (sd.high[idx] - sd.low[idx])
                    .max((sd.high[idx] - prev_close).abs())
                    .max((sd.low[idx] - prev_close).abs());
                p.atr_buf.push_back(tr);
                if p.atr_buf.len() > TURTLE_ATR_PERIOD {
                    p.atr_buf.pop_front();
                }

                let mut exit = false;
                if p.bars_held >= HOLD_MAX {
                    exit = true;
                } else if p.atr_buf.len() >= TURTLE_ATR_PERIOD {
                    let atr = p.atr_buf.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
                    let turtle_stop = p.highest_high - TURTLE_ATR_MULT * atr;
                    if atr > 0.0 && sd.low[idx] <= turtle_stop {
                        exit = true;
                    }
                }

                if exit {
                    let exit_exec = sd.close[idx] * (1.0 - fee);
                    let pct_ret = exit_exec / p.entry_exec - 1.0;
                    if pct_ret > 0.0 {
                        wins += 1;
                    }
                    realized_equity *= 1.0 + p.size * pct_ret;
                    trades += 1;
                    equity_curve.push(realized_equity);
                    continue;
                }
            }
            pos = Some(p);
            equity_curve.push(mark_to_market_equity(realized_equity, &pos, sd, idx, fee));
            continue;
        }

        if live_bot_entry_signal(sd, idx) {
            entry_candidates += 1;
            let btc_pct = btc_atr_percentile(btc, REGIME_ATR_PERIOD, REGIME_LOOKBACK, idx);
            if btc_pct < ATR_RANK_THRESHOLD {
                atr_gate_skips += 1;
                equity_curve.push(realized_equity);
                continue;
            }

            let hedge = hedge_active(btc, idx);
            let mut size = 1.0 / POSITION_CAP as f64;
            if hedge {
                size *= HEDGE_SIZE_MULT;
                hedged_entries += 1;
            }

            pos = Some(PositionState {
                entry_bar: idx,
                entry_exec: sd.close[idx] * (1.0 + fee),
                size,
                highest_high: sd.high[idx],
                bars_held: 0,
                atr_buf: seed_atr_buf(sd, idx),
            });
        }
        equity_curve.push(mark_to_market_equity(realized_equity, &pos, sd, idx, fee));
    }

    if let Some(p) = pos.take() {
        let last_idx = test_end.saturating_sub(1);
        let exit_exec = sd.close[last_idx] * (1.0 - fee);
        let pct_ret = exit_exec / p.entry_exec - 1.0;
        if pct_ret > 0.0 {
            wins += 1;
        }
        realized_equity *= 1.0 + p.size * pct_ret;
        trades += 1;
        if let Some(last) = equity_curve.last_mut() {
            *last = realized_equity;
        }
    }

    let sharpe = annualised_sharpe_from_equity(&equity_curve);
    let max_drawdown = max_dd(&equity_curve);
    let win_rate = if trades > 0 {
        wins as f64 / trades as f64 * 100.0
    } else {
        0.0
    };
    let pass = trades >= MIN_TRADES && sharpe > 0.0;

    WindowResult {
        symbol: symbol.to_string(),
        window,
        train_start: sd
            .dates
            .get(test_start.saturating_sub(TRAIN_BARS))
            .cloned()
            .unwrap_or_default(),
        test_start: sd.dates.get(test_start).cloned().unwrap_or_default(),
        test_end: sd
            .dates
            .get(test_end.saturating_sub(1))
            .cloned()
            .unwrap_or_default(),
        final_equity: realized_equity,
        sharpe,
        max_dd: max_drawdown,
        trades,
        win_rate,
        entry_candidates,
        atr_gate_skips,
        hedged_entries,
        pass,
    }
}

async fn load_symbol(loader: &DataLoader, sym: &str) -> Result<SymData> {
    // Prefer a fresh Binance fetch so the hold-out validation uses the same
    // current-data semantics as `live_bot_exact_equity`. If Binance no longer
    // serves a historical symbol (e.g. delisted/renamed markets), fall back to
    // the local cache and state the truncated date range in the report.
    let df = match loader.fetch_data(sym, "1d", CANDLES).await {
        Ok(df) => df,
        Err(fetch_err) => loader
            .load_from_cache(sym, "1d")
            .with_context(|| {
                format!(
                    "failed to read {} cache after fetch error: {fetch_err}",
                    sym
                )
            })?
            .with_context(|| format!("no cache for {} after fetch error: {fetch_err}", sym))?,
    };
    let close = df
        .column("close")?
        .f64()?
        .into_no_null_iter()
        .collect::<Vec<_>>();
    let high = df
        .column("high")?
        .f64()?
        .into_no_null_iter()
        .collect::<Vec<_>>();
    let low = df
        .column("low")?
        .f64()?
        .into_no_null_iter()
        .collect::<Vec<_>>();
    let time_col = df.column("time").ok();
    let dates: Vec<String> = (0..close.len())
        .map(|i| {
            time_col
                .and_then(|s| s.get(i).ok())
                .map(|v| v.to_string())
                .unwrap_or_default()
        })
        .collect();
    Ok(SymData {
        close,
        high,
        low,
        dates,
    })
}

fn align_pair(symbol_sd: &SymData, btc_sd: &SymData) -> (SymData, SymData) {
    let mut common_dates = symbol_sd.dates.clone();
    common_dates.retain(|d| btc_sd.dates.iter().any(|x| x == d));
    common_dates.sort();
    common_dates.dedup();

    let align = |sd: &SymData| -> SymData {
        let index_by_date: HashMap<String, usize> = sd
            .dates
            .iter()
            .enumerate()
            .map(|(i, d)| (d.clone(), i))
            .collect();
        let mut close = Vec::with_capacity(common_dates.len());
        let mut high = Vec::with_capacity(common_dates.len());
        let mut low = Vec::with_capacity(common_dates.len());
        let mut dates = Vec::with_capacity(common_dates.len());
        for d in &common_dates {
            if let Some(&i) = index_by_date.get(d) {
                close.push(sd.close[i]);
                high.push(sd.high[i]);
                low.push(sd.low[i]);
                dates.push(d.clone());
            }
        }
        SymData {
            close,
            high,
            low,
            dates,
        }
    };

    (align(symbol_sd), align(btc_sd))
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== T80: OOS HOLD-OUT UNIVERSE VALIDATION ===");
    let config = LiveConfig::default();
    let fee = config.fee_pct;
    println!("Hold-outs: {:?}", HOLDOUT_SYMBOLS);
    println!(
        "Exact live Turtle path: EP={}, ATR({}, {:.2}), HOLD_MAX={}, fee={:.2} bps/side",
        TURTLE_EP,
        TURTLE_ATR_PERIOD,
        TURTLE_ATR_MULT,
        HOLD_MAX,
        fee * 10_000.0
    );
    println!(
        "ATR_RANK(AP={}, LB={}, T={:.1}), VOL_LOOKBACK={} unused by bot.rs",
        REGIME_ATR_PERIOD, REGIME_LOOKBACK, ATR_RANK_THRESHOLD, VOL_LOOKBACK
    );

    let loader = DataLoader::new(None, None);
    let btc_raw = load_symbol(&loader, "BTCUSDT").await?;
    let mut results: Vec<WindowResult> = Vec::new();

    for symbol in HOLDOUT_SYMBOLS {
        let raw = load_symbol(&loader, symbol).await?;
        let (sd, btc) = align_pair(&raw, &btc_raw);
        let needed = TRAIN_BARS + WINDOWS * TEST_BARS;
        if sd.close.len() < needed {
            anyhow::bail!(
                "{} has only {} aligned bars; need {}",
                symbol,
                sd.close.len(),
                needed
            );
        }
        let base_start = sd.close.len() - needed;
        println!(
            "{}: {} aligned bars, windows {}..{}",
            symbol,
            sd.close.len(),
            sd.dates[base_start + TRAIN_BARS].clone(),
            sd.dates[sd.close.len() - 1].clone()
        );

        for w in 0..WINDOWS {
            let train_start = base_start + w * TEST_BARS;
            let test_start = train_start + TRAIN_BARS;
            let test_end = test_start + TEST_BARS;
            let res = run_window(symbol, &sd, &btc, test_start, test_end, w + 1, fee);
            println!(
                "  W{} {}..{}: eq {:.3}x, Sharpe {:.2}, DD {:.1}%, trades {}, pass {}",
                res.window,
                res.test_start,
                res.test_end,
                res.final_equity,
                res.sharpe,
                res.max_dd,
                res.trades,
                res.pass
            );
            results.push(res);
        }
    }

    let total = results.len();
    let passes = results.iter().filter(|r| r.pass).count();
    let avg_sharpe = results.iter().map(|r| r.sharpe).sum::<f64>() / total as f64;
    let avg_return =
        results.iter().map(|r| r.final_equity - 1.0).sum::<f64>() / total as f64 * 100.0;
    let avg_dd = results.iter().map(|r| r.max_dd).sum::<f64>() / total as f64;
    let total_trades = results.iter().map(|r| r.trades).sum::<usize>();
    let pass_rate = passes as f64 / total as f64 * 100.0;

    println!("--- T80 Global ---");
    println!("Pass rate: {}/{} ({:.1}%)", passes, total, pass_rate);
    println!("Avg Sharpe: {:.3}", avg_sharpe);
    println!("Avg return/window: {:.1}%", avg_return);
    println!("Avg MaxDD: {:.1}%", avg_dd);
    println!("Trades: {}", total_trades);

    let verdict = if pass_rate >= 70.0 && avg_sharpe >= 0.5 {
        "PASS: hold-out symbols support out-of-sample generalization."
    } else if pass_rate < 60.0 {
        "FAIL: live Turtle path generalizes poorly to these hold-out symbols; production edge remains universe-dependent."
    } else {
        "MIXED: hold-out evidence is borderline; do not treat as clean generalization."
    };
    println!("Verdict: {}", verdict);

    let mut csv = String::from("symbol,window,train_start,test_start,test_end,final_equity,return_pct,sharpe,max_dd,trades,win_rate,entry_candidates,atr_gate_skips,hedged_entries,pass\n");
    for r in &results {
        csv.push_str(&format!(
            "{},{},{},{},{},{:.10},{:.4},{:.6},{:.4},{},{:.2},{},{},{},{}\n",
            r.symbol,
            r.window,
            r.train_start,
            r.test_start,
            r.test_end,
            r.final_equity,
            (r.final_equity - 1.0) * 100.0,
            r.sharpe,
            r.max_dd,
            r.trades,
            r.win_rate,
            r.entry_candidates,
            r.atr_gate_skips,
            r.hedged_entries,
            r.pass
        ));
    }
    File::create("snapshots/t80_oos_universe_validation.csv")?.write_all(csv.as_bytes())?;

    let mut md = File::create("snapshots/t80_oos_universe_validation.md")?;
    writeln!(md, "# T80: OOS Universe Validation\n")?;
    writeln!(
        md,
        "Generated: {}\n",
        Utc::now().format("%Y-%m-%d %H:%M UTC")
    )?;
    writeln!(md, "## Scope\n")?;
    writeln!(md, "Hold-out symbols: **UNIUSDT, MATICUSDT, AVAXUSDT**. These pairs were reserved because they are outside the repeatedly optimized 9-universe grid. BTCUSDT is used only for ATR_RANK/hedge regime gates, not as a traded symbol.\n")?;
    writeln!(md, "## Method\n")?;
    writeln!(
        md,
        "- Six most recent walk-forward windows per symbol: 252 train bars + 252 test bars."
    )?;
    writeln!(md, "- Entry/exit mirrors `examples/live_bot_exact_equity.rs` and the as-coded daily `src/live/bot.rs` Turtle path.")?;
    writeln!(
        md,
        "- Entry: current-inclusive Turtle EP={} window, equality allowed.",
        TURTLE_EP
    )?;
    writeln!(
        md,
        "- Gate: ATR_RANK(AP={}, LB={}, T={:.1}); hedge size multiplier {:.2} when active.",
        REGIME_ATR_PERIOD, REGIME_LOOKBACK, ATR_RANK_THRESHOLD, HEDGE_SIZE_MULT
    )?;
    writeln!(md, "- Exit: Turtle ATR-only (`highest_high - {:.2}×ATR{}`) plus HOLD_MAX={}; fee {:.2} bps/side.", TURTLE_ATR_MULT, TURTLE_ATR_PERIOD, HOLD_MAX, fee * 10_000.0)?;
    writeln!(
        md,
        "- `VOL_LOOKBACK={}` remains unused by exact live bot entry logic.\n",
        VOL_LOOKBACK
    )?;
    writeln!(md, "## Global Result\n")?;
    writeln!(md, "| Metric | Value |")?;
    writeln!(md, "|---|---:|")?;
    writeln!(
        md,
        "| Pass rate | {}/{} ({:.1}%) |",
        passes, total, pass_rate
    )?;
    writeln!(md, "| Avg Sharpe | {:.3} |", avg_sharpe)?;
    writeln!(md, "| Avg return/window | {:.1}% |", avg_return)?;
    writeln!(md, "| Avg MaxDD | {:.1}% |", avg_dd)?;
    writeln!(md, "| Trades | {} |\n", total_trades)?;
    writeln!(md, "**Verdict:** {}\n", verdict)?;

    writeln!(md, "## Window Results\n")?;
    writeln!(
        md,
        "| Symbol | W | Test Start | Test End | Equity | Sharpe | MaxDD | Trades | Pass |"
    )?;
    writeln!(md, "|---|---:|---|---|---:|---:|---:|---:|---|")?;
    for r in &results {
        writeln!(
            md,
            "| {} | {} | {} | {} | {:.3}x | {:.2} | {:.1}% | {} | {} |",
            r.symbol,
            r.window,
            r.test_start,
            r.test_end,
            r.final_equity,
            r.sharpe,
            r.max_dd,
            r.trades,
            if r.pass { "PASS" } else { "FAIL" }
        )?;
    }

    writeln!(md, "\n## Per-Symbol Summary\n")?;
    writeln!(
        md,
        "| Symbol | Pass | Avg Equity | Avg Sharpe | Avg DD | Trades |"
    )?;
    writeln!(md, "|---|---:|---:|---:|---:|---:|")?;
    for symbol in HOLDOUT_SYMBOLS {
        let rows: Vec<&WindowResult> = results.iter().filter(|r| r.symbol == symbol).collect();
        let n = rows.len() as f64;
        let p = rows.iter().filter(|r| r.pass).count();
        let eq = rows.iter().map(|r| r.final_equity).sum::<f64>() / n;
        let s = rows.iter().map(|r| r.sharpe).sum::<f64>() / n;
        let d = rows.iter().map(|r| r.max_dd).sum::<f64>() / n;
        let t = rows.iter().map(|r| r.trades).sum::<usize>();
        writeln!(
            md,
            "| {} | {}/{} | {:.3}x | {:.2} | {:.1}% | {} |",
            symbol,
            p,
            rows.len(),
            eq,
            s,
            d,
            t
        )?;
    }
    writeln!(md, "\nPass definition: ≥{} trades and Sharpe > 0.0 in the test window. Promotion guardrail from PLAN: global pass rate ≥70% and avg Sharpe ≥0.5.\n", MIN_TRADES)?;
    writeln!(md, "## Files\n")?;
    writeln!(
        md,
        "- `snapshots/t80_oos_universe_validation.csv` — per-window data"
    )?;

    println!("CSV: snapshots/t80_oos_universe_validation.csv");
    println!("Report: snapshots/t80_oos_universe_validation.md");
    Ok(())
}
