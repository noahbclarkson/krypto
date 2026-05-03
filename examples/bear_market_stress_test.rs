//!
//! **Bear Market Stress Test — 2018-2019 Grinding Bear**
//!
//! Tests Turtle against buy-hold during the worst sustained bear market in crypto history.
//! BTC fell -82% over 12 months (Jan–Dec 2018), with grinding partial recoveries in 2019.
//!
//! Key questions:
//! 1. Does Turtle's ATR trailing stop protect capital in a grinding bear?
//! 2. Does it outperform buy-hold through sheer drawdown reduction?
//! 3. Or does repeated whipsaw erosion make it barely better than buy-hold?
//!
//! Run: cargo run --example bear_market_stress_test --profile sweep
//!
use anyhow::{Context, Result};
use chrono::NaiveDate;
use polars::prelude::*;
use std::path::Path;

fn load_parquet(symbol: &str, interval: &str) -> Result<DataFrame> {
    let path = Path::new("data/cache").join(format!(
        "{}_{}.parquet",
        symbol.to_lowercase(),
        interval
    ));
    LazyFrame::scan_parquet(&path, Default::default())?
        .collect()
        .context(format!("load_parquet failed: {:?}", path))
}

fn max_dd(equity: &[f64]) -> f64 {
    let mut peak = equity[0];
    let mut m: f64 = 0.0;
    for &eq in equity {
        peak = peak.max(eq);
        m = m.max((peak - eq) / peak);
    }
    m
}

fn ann_sharpe(returns: &[f64]) -> f64 {
    if returns.is_empty() {
        return 0.0;
    }
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let variance =
        returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64;
    let std = variance.sqrt();
    if std == 0.0 {
        return 0.0;
    }
    mean / std * (252.0_f64).sqrt()
}

fn equity_curve(init: f64, returns: &[f64]) -> Vec<f64> {
    let mut eq = Vec::with_capacity(returns.len() + 1);
    eq.push(init);
    let mut c = init;
    for r in returns {
        c *= 1.0 + r;
        eq.push(c);
    }
    eq
}

fn main() -> Result<()> {
    println!("\n📉 Bear Market Stress Test — 2018-2019 Grinding Bear\n");
    println!("{}", "═".repeat(62));

    // ── Load BTC 1d from parquet ───────────────────────────────────────────
    let btc = load_parquet("BTCUSDT", "1d")?;
    let times_col = btc.column("time")?.datetime()?;
    let closes_col = btc.column("close")?.f64()?;
    let highs_col = btc.column("high")?.f64()?;
    let lows_col = btc.column("low")?.f64()?;

    let all_times: Vec<i64> = times_col.into_iter().map(|t| t.unwrap()).collect();
    let all_closes: Vec<f64> = closes_col.into_iter().map(|t| t.unwrap()).collect();
    let all_highs: Vec<f64> = highs_col.into_iter().map(|t| t.unwrap()).collect();
    let all_lows: Vec<f64> = lows_col.into_iter().map(|t| t.unwrap()).collect();

    // ── Window: 2018-01-01 to 2019-12-31 ────────────────────────────────────
    let start_2018 = NaiveDate::from_ymd_opt(2018, 1, 1).unwrap();
    let end_2019 = NaiveDate::from_ymd_opt(2019, 12, 31).unwrap();

    // Binary search for start/end index
    fn find_idx(times: &[i64], target: NaiveDate) -> Option<usize> {
        times.iter().position(|&t| {
            let d = chrono::DateTime::from_timestamp_millis(t)
                .map(|dt| dt.date_naive());
            d == Some(target)
        })
    }

    let si = find_idx(&all_times, start_2018).unwrap_or(0);
    let ei = find_idx(&all_times, end_2019).unwrap_or(all_times.len() - 1);
    let n = ei - si + 1;
    println!(
        "Window: {} bars (2018-01-01 → 2019-12-31)\n",
        n
    );

    // ── Params (production Turtle-only, no regime filter) ──────────────────
    const EP: usize = 21;
    const ATR_P: usize = 24;
    const ATR_M: f64 = 2.0;
    const HOLD_MAX: usize = 12;
    const FEE: f64 = 0.0004;
    const INIT: f64 = 10_000.0;

    // ── Simulate ─────────────────────────────────────────────────────────────
    let mut cash = INIT;
    let mut position: Option<(f64, f64, usize)> = None; // (qty, entry_px, bars_held)
    let mut close_buf: Vec<f64> = Vec::with_capacity(EP + 10);
    let mut atr_buf: Vec<f64> = Vec::with_capacity(ATR_P);
    let mut turtle_returns: Vec<f64> = Vec::new();
    let mut turtle_eq = Vec::with_capacity(n);
    let mut bh_eq = Vec::with_capacity(n);

    let mut trades = 0usize;
    let mut wins = 0usize;
    let prev_bh_price = all_closes[si];

    let bh_qty = INIT / all_closes[si];

    for i in 0..n {
        let close = all_closes[si + i];
        let high = all_highs[si + i];
        let low = all_lows[si + i];
        let _prev_close = if i == 0 { close } else { all_closes[si + i - 1] };

        // Buy-hold equity (independent — does NOT use Turtle's cash/position)
        bh_eq.push(bh_qty * close);

        // ── Turtle ATR ───────────────────────────────────────────────────────
        let tr = high - low;
        atr_buf.push(tr);
        if atr_buf.len() > ATR_P {
            atr_buf.remove(0);
        }
        let atr = if atr_buf.len() == ATR_P {
            atr_buf.iter().sum::<f64>() / ATR_P as f64
        } else {
            tr
        };

        // ── Entry ──────────────────────────────────────────────────────────
        close_buf.push(close);
        if close_buf.len() > EP + 1 {
            close_buf.remove(0);
        }

        if position.is_none() && close_buf.len() >= EP + 1 {
            // max of prior EP closes (before current bar — exclude current close)
            let end = close_buf.len() - 1; // exclude current bar
            let start = end.saturating_sub(EP);
            let max_prev = close_buf[start..end]
                .iter()
                .fold(0.0f64, |m, c| m.max(*c));
            if close > max_prev {
                let qty = cash / close;
                cash -= qty * close * (1.0 + FEE);
                position = Some((qty, close, 0));
            }
        }

        // ── Exit ────────────────────────────────────────────────────────────
        if let Some((qty, entry_px, bars_held)) = position {
            let new_bars = bars_held + 1;
            let mut exited = false;

            // Turtle ATR trailing stop
            if atr_buf.len() == ATR_P {
                let trail = high - ATR_M * atr;
                if close <= trail {
                    cash += qty * close * (1.0 - FEE);
                    let ret = (close - entry_px) / entry_px;
                    turtle_returns.push(ret);
                    if ret > 0.0 { wins += 1; }
                    trades += 1;
                    exited = true;
                }
            }

            // Hold-timeout
            if !exited && new_bars >= HOLD_MAX {
                cash += qty * close * (1.0 - FEE);
                let ret = (close - entry_px) / entry_px;
                turtle_returns.push(ret);
                if ret > 0.0 { wins += 1; }
                trades += 1;
                exited = true;
            }

            if !exited {
                position = Some((qty, entry_px, new_bars));
            } else {
                position = None;
            }
        }

        let pos_val = position.map(|(q, _, _)| q * close).unwrap_or(0.0);
        turtle_eq.push(cash + pos_val);
    }

    // ── BH returns (buy at bar 0, hold to end) ──────────────────────────────
    let mut bh_returns = Vec::<f64>::new();
    for i in 0..n {
        let prev = if i == 0 { prev_bh_price } else { all_closes[si + i - 1] };
        bh_returns.push((all_closes[si + i] - prev) / prev);
    }

    let final_turtle = turtle_eq.last().copied().unwrap_or(INIT);
    let final_bh = bh_eq.last().copied().unwrap_or(INIT);
    let turtle_sharpe = ann_sharpe(&turtle_returns);
    let bh_sharpe = ann_sharpe(&bh_returns);
    let turtle_max_dd = max_dd(&turtle_eq);
    let bh_max_dd = max_dd(&bh_eq);

    // ── Results ─────────────────────────────────────────────────────────────
    println!("{}", "─".repeat(62));
    println!("{:>30} {:>15} {:>15}", "Metric", "Turtle", "Buy-Hold");
    println!("{}", "─".repeat(62));
    println!(
        "{:>30} {:>15.2}x {:>15.2}x",
        "Final Equity (10k)",
        final_turtle / INIT,
        final_bh / INIT
    );
    println!(
        "{:>30} {:>+15.1}% {:>+15.1}%",
        "Total Return",
        (final_turtle / INIT - 1.0) * 100.0,
        (final_bh / INIT - 1.0) * 100.0
    );
    println!(
        "{:>30} {:>15.2} {:>15.2}",
        "Annualized Sharpe",
        turtle_sharpe,
        bh_sharpe
    );
    println!(
        "{:>30} {:>15.1}% {:>15.1}%",
        "Max Drawdown",
        turtle_max_dd * 100.0,
        bh_max_dd * 100.0
    );
    println!("{:>30} {:>15} {:>15}", "Trades", trades, "-");
    println!(
        "{:>30} {:>15.1}% {:>15.1}%",
        "Win Rate",
        if trades > 0 {
            wins as f64 / trades as f64 * 100.0
        } else {
            0.0
        },
        0.0
    );
    println!("{}", "─".repeat(62));

    // ── Per-Year Breakdown ────────────────────────────────────────────────────
    println!("\nPer-Year Breakdown:");
    println!("{}", "─".repeat(62));

    for (year, yr_start, yr_end) in [
        (
            2018,
            NaiveDate::from_ymd_opt(2018, 1, 1).unwrap(),
            NaiveDate::from_ymd_opt(2018, 12, 31).unwrap(),
        ),
        (
            2019,
            NaiveDate::from_ymd_opt(2019, 1, 1).unwrap(),
            NaiveDate::from_ymd_opt(2019, 12, 31).unwrap(),
        ),
    ] {
        let mut yi_start = None;
        let mut yi_end = None;
        for (i, t) in all_times[si..ei + 1].iter().enumerate() {
            if let Some(dt) = chrono::DateTime::from_timestamp_millis(*t) {
                let d = dt.date_naive();
                if d == yr_start && yi_start.is_none() {
                    yi_start = Some(si + i);
                }
                if d == yr_end {
                    yi_end = Some(si + i);
                }
            }
        }

        if let (Some(s), Some(e)) = (yi_start, yi_end) {
            let n_yr = (e - s + 1).min(turtle_eq.len().saturating_sub(s - si));
            if n_yr <= 0 {
                continue;
            }
            let t_start = s - si;
            let t_end = t_start + n_yr;
            let yr_turtle = &turtle_eq[t_start..t_end];
            let yr_bh = &bh_eq[t_start..t_end];
            if yr_turtle.is_empty() {
                continue;
            }
            let yr_ret_t =
                (*yr_turtle.last().unwrap() / *yr_turtle.first().unwrap() - 1.0) * 100.0;
            let yr_ret_b = (*yr_bh.last().unwrap() / *yr_bh.first().unwrap() - 1.0) * 100.0;
            let yr_dd_t = max_dd(yr_turtle) * 100.0;
            let yr_dd_b = max_dd(yr_bh) * 100.0;
            let ratio = if yr_ret_b.abs() > 0.01 {
                yr_ret_t / yr_ret_b
            } else {
                0.0
            };
            println!(
                "  {:>4} | Turtle: {:>+7.1}% ({:>+6.1}% DD) | BH: {:>+8.1}% ({:>+7.1}% DD) | {:.2}x Turtle",
                year, yr_ret_t, yr_dd_t, yr_ret_b, yr_dd_b, ratio,
            );
        }
    }

    // ── Verdict ───────────────────────────────────────────────────────────────
    println!("\n{}", "═".repeat(62));
    println!("VERDICT");
    println!("{}", "═".repeat(62));

    let turtle_wins = final_turtle > final_bh;
    let turtle_less_dd = turtle_max_dd < bh_max_dd;
    let turtle_better_sharpe = turtle_sharpe > bh_sharpe;

    if turtle_wins && turtle_less_dd && turtle_better_sharpe {
        println!("  ✅ Turtle DOMINATES buy-hold on ALL three metrics");
    } else if turtle_wins && turtle_less_dd {
        println!("  ✅ Turtle wins on return AND drawdown");
    } else if turtle_wins {
        println!("  ⚠️  Turtle wins on return but loses on drawdown");
    } else {
        println!("  🔴 Turtle LOSES to buy-hold on return");
    }

    if turtle_max_dd > 0.50 {
        println!("  ⚠️  Turtle MaxDD > 50% — ATR stop didn't prevent grinding drawdown");
    }
    if turtle_sharpe < bh_sharpe {
        println!(
            "  ⚠️  Turtle Sharpe ({:.2}) < BH Sharpe ({:.2}) — risk-adj returns worse",
            turtle_sharpe, bh_sharpe
        );
    }

    println!(
        "\n  Edge over buy-hold: {:+.1}%",
        (final_turtle - final_bh).abs() / final_bh.abs() * 100.0
    );
    println!("\n  Honest interpretation:");
    println!("  • All metrics are SIMULATION UPPER BOUNDS");
    println!("  • Execution: 0.04% taker modeled");
    println!("  • No regime filter — pure Turtle entry + ATR trailing stop");
    println!("  • 2018-2019 is the stress regime; 2022 crash was faster/easier to manage");

    Ok(())
}
