//! Turtle+MACD Longer Hold Walk-Forward — Kira Research 2026-04-01
//!
//! Root cause hypothesis: 21-bar hold is too short for crypto trend following.
//! 21-bar Donchian lookback fires entries near local highs; subsequent 21-bar hold
//! is too short to let winners run through the short-term reversals that stop them out.
//!
//! Test: hold periods of 21, 42, 60, 90 bars on Turtle+MACD (no vol-scaling).
//! Same walk-forward design (252 train / 252 test / top-3 MACD-gap-ranked / 0.1% taker).

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;

const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const CANDLES: u32 = 3000;
const TURTLE_ENTRY: usize = 20;
const HOLDS: [usize; 4] = [21, 42, 60, 90];

#[tokio::main]
async fn main() -> Result<()> {
    println!("═══ Turtle+MACD Longer Hold Walk-Forward ═══\n");

    let loader = DataLoader::new(None, None);
    let syms = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"];

    let mut cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for s in &syms {
        let raw = loader.fetch_with_cache(s, "1d", CANDLES).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        min_len = min_len.min(df.height());
        cache.insert(s.to_string(), df);
    }

    let n = min_len.min(2800);
    for (_, df) in &mut cache {
        if df.height() > n {
            *df = df.slice(0, n);
        }
    }

    let total_windows = n.saturating_sub(TRAIN_BARS) / TEST_BARS;
    println!(
        "{} syms, {} bars, {} windows, holds {:?}\n",
        syms.len(),
        n,
        total_windows,
        HOLDS
    );

    // ── Per-hold accumulators ───────────────────────────────────────────────
    let mut pass_cnt = vec![0usize; HOLDS.len()];
    let mut total_ret = vec![0.0f64; HOLDS.len()];
    let mut total_sh = vec![0.0f64; HOLDS.len()];
    let mut total_dd = vec![0.0f64; HOLDS.len()];
    let mut worst_dd = vec![0.0f64; HOLDS.len()];
    let mut total_trades = vec![0usize; HOLDS.len()];

    // H2H wins vs hold=21 for each longer hold
    let mut h2h_wins: Vec<usize> = vec![0usize; HOLDS.len()];

    // ── Per-window loop ───────────────────────────────────────────────────
    for wi in 0..total_windows {
        let train_end = TRAIN_BARS + wi * TEST_BARS;
        let tstart = train_end;
        let tend = (tstart + TEST_BARS).min(n);
        let max_hold = *HOLDS.iter().max().unwrap();
        if tend - tstart < max_hold + 2 {
            continue;
        }

        let mut close: HashMap<String, Vec<f64>> = HashMap::new();
        let mut macd_line: HashMap<String, Vec<f64>> = HashMap::new();
        let mut macd_signal: HashMap<String, Vec<f64>> = HashMap::new();
        let mut dc_hi: HashMap<String, Vec<f64>> = HashMap::new();
        let mut dc_lo: HashMap<String, Vec<f64>> = HashMap::new();

        for sym in &syms {
            let df = cache.get(*sym).unwrap();
            let c = col_f64(df, "close")?;
            let h = col_f64(df, "high")?;
            let l = col_f64(df, "low")?;
            close.insert(sym.to_string(), c);
            macd_line.insert(sym.to_string(), col_f64(df, "macd")?);
            macd_signal.insert(sym.to_string(), col_f64(df, "macd_signal")?);
            dc_hi.insert(sym.to_string(), rolling_max(&h, TURTLE_ENTRY));
            dc_lo.insert(sym.to_string(), rolling_min(&l, TURTLE_ENTRY));
        }

        // Run each hold period
        let mut window_results: Vec<(f64, f64, f64, usize)> = Vec::new();
        for (hi, &hold) in HOLDS.iter().enumerate() {
            let (ret, sh, dd, trd) = run_backtest(
                &close,
                &macd_line,
                &macd_signal,
                &dc_hi,
                &dc_lo,
                &syms[..],
                tstart,
                tend,
                hold,
            );
            let pass = trd >= MIN_TRADES && ret > 0.0;
            if pass {
                pass_cnt[hi] += 1;
            }
            total_ret[hi] += ret;
            total_sh[hi] += sh;
            total_dd[hi] += dd;
            worst_dd[hi] = worst_dd[hi].min(dd);
            total_trades[hi] += trd;
            window_results.push((ret, dd, sh, trd));

            // H2H vs hold=21
            if hi > 0 {
                let base_ret = window_results[0].0;
                if trd >= MIN_TRADES && ret > base_ret {
                    h2h_wins[hi] += 1;
                }
            }
        }

        // Print one summary row per window (all holds on one line)
        let pass_str: Vec<String> = window_results
            .iter()
            .map(|(ret, dd, sh, trd)| {
                let ok = *trd >= MIN_TRADES && *ret > 0.0;
                format!(
                    "{:>+7.0}% {:5.2} {:5.0}% {:3} {}",
                    ret,
                    sh,
                    -dd,
                    trd,
                    if ok { "✓" } else { "✗" }
                )
            })
            .collect();
        println!("W{:02} b{} | {}", wi, tstart, pass_str.join(" | "));

        drop(close);
        drop(macd_line);
        drop(macd_signal);
        drop(dc_hi);
        drop(dc_lo);
    }

    let nw = total_windows;
    if nw == 0 {
        return Ok(());
    }

    // ── Summary ───────────────────────────────────────────────────────────
    println!("\n══════════════════════════════════════════════════════════════");
    println!("SUMMARY ({} windows)\n", nw);

    // Header
    let hold_labels: Vec<String> = HOLDS
        .iter()
        .map(|h| format!("{:>28}", format!("HOLD {:02}", h)))
        .collect();
    println!(
        "{:<6} {:>5} {:>28} {:>28} {:>28} {:>28}",
        "", "", hold_labels[0], hold_labels[1], hold_labels[2], hold_labels[3]
    );
    println!("{:<6} {:>5} {:>8} {:>5} {:>6} {:>4} {:>8} {:>5} {:>6} {:>4} {:>8} {:>5} {:>6} {:>4} {:>8} {:>5} {:>6} {:>4}",
        "Hold", "Pass", "ret%", "sh", "DD%", "Trd",
        "ret%", "sh", "DD%", "Trd",
        "ret%", "sh", "DD%", "Trd",
        "ret%", "sh", "DD%", "Trd");
    println!("{}", "-".repeat(130));

    let nn = nw as f64;
    let mut summary_rows: Vec<(usize, f64, f64, f64, f64, usize)> = Vec::new();
    for (hi, &hold) in HOLDS.iter().enumerate() {
        let avg_ret = total_ret[hi] / nn;
        let avg_sh = total_sh[hi] / nn;
        let avg_dd = total_dd[hi] / nn;
        summary_rows.push((hold, avg_ret, avg_sh, avg_dd, worst_dd[hi], pass_cnt[hi]));
    }

    // Print one row per hold (vs the base 21 hold)
    let base = &summary_rows[0];
    println!(
        "{:04}:  {}/{} {:>+8.0}% {:5.2} {:5.0}% {:4} trades",
        base.0, base.5, nw, base.1, base.2, -base.3, total_trades[0]
    );
    for (hi, row) in summary_rows.iter().enumerate().skip(1) {
        let vs21 = if row.5 >= MIN_TRADES && row.1 > base.1 {
            "▲"
        } else if row.5 >= MIN_TRADES && row.1 < base.1 {
            "▼"
        } else {
            " "
        };
        println!(
            "{:04}:  {}/{} {:>+8.0}% {:5.2} {:5.0}% {:4} trades  h2h {} {} {}",
            row.0, row.5, nw, row.1, row.2, -row.3, total_trades[hi], h2h_wins[hi], vs21, nw
        );
    }

    // ── Honest Assessment ─────────────────────────────────────────────────
    println!("\n─── Honest Assessment ───");
    let base_pass = pass_cnt[0];
    let base_avg_dd = total_dd[0] / nn;
    let base_worst_dd = worst_dd[0];

    for (hi, &hold) in HOLDS.iter().enumerate().skip(1) {
        let longer_pass = pass_cnt[hi];
        let longer_avg_dd = total_dd[hi] / nn;
        let longer_worst_dd = worst_dd[hi];
        let h2h_wr = h2h_wins[hi] as f64 / nn as f64;
        let avg_ret_improvement = (total_ret[hi] - total_ret[0]) / nn;

        println!("\nHold {} vs hold 21 (baseline):", hold);
        println!(
            "  Pass rate:   {} → {} ({}{})",
            base_pass,
            longer_pass,
            if longer_pass > base_pass { "+" } else { "" },
            longer_pass as i32 - base_pass as i32
        );
        println!(
            "  Avg DD:      {:.1}% → {:.1}% ({}{:.1}pp)",
            -base_avg_dd,
            -longer_avg_dd,
            if longer_avg_dd < base_avg_dd {
                "▼ "
            } else {
                "▲ "
            },
            (longer_avg_dd - base_avg_dd).abs()
        );
        println!(
            "  Worst DD:    {:.1}% → {:.1}%",
            -base_worst_dd, -longer_worst_dd
        );
        println!(
            "  Avg ret:     {:+.1}% → {:+.1}% ({:+.1}pp)",
            total_ret[0] / nn,
            total_ret[hi] / nn,
            avg_ret_improvement
        );
        println!(
            "  H2H vs H21:  {}/{} windows ({:.0}%)",
            h2h_wins[hi],
            nw,
            h2h_wr * 100.0
        );

        // Decision
        let dd_improved = longer_avg_dd < base_avg_dd;
        let worst_improved = longer_worst_dd < base_worst_dd;
        let pass_ok = longer_pass >= base_pass;
        let h2h_ok = h2h_wr > 0.5;

        if dd_improved && worst_improved && pass_ok {
            println!(
                "  ★★★ Hold {} is the better candidate for live sizing",
                hold
            );
        } else if dd_improved && pass_ok && h2h_wr >= 0.4 {
            println!(
                "  ★★  Hold {} reduces DD — consider as risk-reduced variant",
                hold
            );
        } else if h2h_wr >= 0.5 && avg_ret_improvement > 50.0 {
            println!(
                "  ★   Hold {} wins more in backtest but DD may be structural",
                hold
            );
        } else {
            println!(
                "  ✗   Hold {} does NOT improve DD or pass rate — reject structural change",
                hold
            );
        }
    }

    println!("\n─── Structural insight ───");
    println!(
        "If longer holds reduce DD without destroying pass rate → MaxDD is partially hold-driven."
    );
    println!("If longer holds reduce DD but also reduce returns proportionally → trade-off, not solution.");
    println!("If longer holds destroy pass rate → 21-bar is optimal; DD problem is elsewhere (entry signal, regime).");

    Ok(())
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn col_f64(df: &DataFrame, name: &str) -> Result<Vec<f64>> {
    use polars::chunked_array::ChunkedArray;
    let ca = df.column(name)?.f64()?;
    Ok(ca.into_iter().map(|v| v.unwrap_or(0.0)).collect())
}

fn rolling_max(high: &[f64], window: usize) -> Vec<f64> {
    let n = high.len();
    let mut out = vec![0.0; n];
    for i in window..n {
        out[i] = high[(i - window)..i]
            .iter()
            .fold(high[i - window], |a, &b| a.max(b));
    }
    out
}

fn rolling_min(low: &[f64], window: usize) -> Vec<f64> {
    let n = low.len();
    let mut out = vec![0.0; n];
    for i in window..n {
        out[i] = low[(i - window)..i]
            .iter()
            .fold(low[i - window], |a, &b| a.min(b));
    }
    out
}

fn sma(close: &[f64], window: usize) -> f64 {
    if close.len() < window {
        return close.iter().sum::<f64>() / close.len().max(1) as f64;
    }
    close[close.len() - window..].iter().sum::<f64>() / window as f64
}

// ─── Backtest (parametrized hold) ──────────────────────────────────────────────

fn run_backtest(
    close: &HashMap<String, Vec<f64>>,
    macd_line: &HashMap<String, Vec<f64>>,
    macd_signal: &HashMap<String, Vec<f64>>,
    dc_hi: &HashMap<String, Vec<f64>>,
    dc_lo: &HashMap<String, Vec<f64>>,
    syms: &[&str],
    tstart: usize,
    tend: usize,
    hold_bars: usize,
) -> (f64, f64, f64, usize) {
    let mut rets: Vec<f64> = Vec::new();

    let mut bar = tstart;
    while bar + hold_bars < tend {
        // BTC regime filter
        let btc_close = match close.get("BTCUSDT") {
            Some(c) if c.len() > bar => &c[..],
            _ => {
                bar += 1;
                continue;
            }
        };
        let btc_sma200 = sma(btc_close, 200);
        let btc_above_sma = btc_close[bar] > btc_sma200;

        #[derive(Clone)]
        struct Entry {
            sym: String,
            direction: i8,
        }
        let mut longs: Vec<Entry> = Vec::new();
        let mut shorts: Vec<Entry> = Vec::new();

        for sym in syms {
            let cl = match close.get(*sym) {
                Some(c) if c.len() > bar + hold_bars => c,
                _ => continue,
            };
            let price = cl[bar];
            let ml = *macd_line.get(*sym).and_then(|v| v.get(bar)).unwrap_or(&0.0);
            let ms = *macd_signal
                .get(*sym)
                .and_then(|v| v.get(bar))
                .unwrap_or(&0.0);
            let dhi = *dc_hi.get(*sym).and_then(|v| v.get(bar)).unwrap_or(&0.0);
            let dlo = *dc_lo.get(*sym).and_then(|v| v.get(bar)).unwrap_or(&0.0);

            // Turtle + MACD confirm + BTC regime
            if price > dhi && ml > ms && btc_above_sma {
                longs.push(Entry {
                    sym: (*sym).to_string(),
                    direction: 1,
                });
            } else if price < dlo && ml < ms && !btc_above_sma {
                shorts.push(Entry {
                    sym: (*sym).to_string(),
                    direction: -1,
                });
            }
        }

        // Rank by MACD gap (strength of signal)
        let mut long_entries: Vec<_> = longs
            .into_iter()
            .map(|e| {
                let ml = macd_line
                    .get(&e.sym)
                    .and_then(|v| v.get(bar))
                    .copied()
                    .unwrap_or(0.0);
                let ms = macd_signal
                    .get(&e.sym)
                    .and_then(|v| v.get(bar))
                    .copied()
                    .unwrap_or(0.0);
                (e.sym, e.direction, ml - ms)
            })
            .collect();
        let mut short_entries: Vec<_> = shorts
            .into_iter()
            .map(|e| {
                let ml = macd_line
                    .get(&e.sym)
                    .and_then(|v| v.get(bar))
                    .copied()
                    .unwrap_or(0.0);
                let ms = macd_signal
                    .get(&e.sym)
                    .and_then(|v| v.get(bar))
                    .copied()
                    .unwrap_or(0.0);
                (e.sym, e.direction, ms - ml)
            })
            .collect();

        long_entries.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
        short_entries.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());

        let top_longs: Vec<_> = long_entries.into_iter().take(3).collect();
        let top_shorts: Vec<_> = short_entries.into_iter().take(3).collect();

        if !top_longs.is_empty() || !top_shorts.is_empty() {
            for (sym, dir, _) in top_longs.into_iter().chain(top_shorts.into_iter()) {
                if let Some(cl) = close.get(&sym) {
                    if bar + 1 < cl.len() && bar + hold_bars < cl.len() {
                        let entry = cl[bar + 1];
                        let exit = cl[bar + hold_bars];
                        let gross = exit / entry - 1.0;
                        let net = if dir > 0 {
                            gross - 2.0 * TAKER_FEE
                        } else {
                            -(gross + 2.0 * TAKER_FEE)
                        };
                        rets.push(net);
                    }
                }
            }
        }

        bar += 1;
    }

    if rets.is_empty() {
        return (0.0, 0.0, 0.0, 0);
    }

    // Compound equity + drawdown
    let mut equity: f64 = 1.0;
    let mut peak: f64 = 1.0;
    let mut max_dd: f64 = 0.0;
    for &r in &rets {
        equity *= 1.0 + r;
        peak = peak.max(equity);
        let dd = (peak - equity) / peak;
        max_dd = max_dd.max(dd);
    }

    let ret = (equity - 1.0) * 100.0;
    let n = rets.len() as f64;
    let mean = rets.iter().sum::<f64>() / n;
    let var = rets
        .iter()
        .map(|&r| {
            let d = r - mean;
            d * d
        })
        .sum::<f64>()
        / n;
    let std = var.sqrt();
    let sharpe = if std > 1e-9 {
        mean / std * (252.0_f64.sqrt())
    } else {
        0.0
    };

    (ret, sharpe, max_dd * 100.0, rets.len())
}
