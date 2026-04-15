//! Chandelier ATR Trailing Exit Walk-Forward Validation
//!
//! PURPOSE: The fixed 54-bar hold has been the execution model since 2026-04-08.
//! Chandelier ATR exit was proposed 2026-04-05 and NEVER successfully built.
//! This is the single most overdue structural improvement in the execution model.
//!
//! Chandelier Exit (Louden, 2005):
//!   Long exit: when price closes below (highest_high_since_entry - mult * ATR_period)
//!
//! Test design:
//!   - Compare Fixed54 hold (baseline) vs 4 Chandelier multipliers (2.5, 3.0, 3.5, 4.0)
//!   - All use same A/D Momentum signal (AD_PERIOD=47, validated in prior sessions)
//!   - Walk-forward: 252-bar train / 252-bar test, 4 chronological windows
//!   - 9 universes (Base5, NoDOGE, Legacy4, Legacy5BNB, OldGuardNoBNB, LargeCaps5, Legacy3, LowVolume5, OldGuard4)
//!   - 0.1% taker each side, next-bar open entry
//!
//! Success criteria: Chandelier must beat Fixed54 on OOS Sharpe or MaxDD, not just train-set metrics.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)
const HOLD_FIXED: usize = 54;
const CHANDELIER_PERIOD: usize = 45;
const CHANDELIER_MULTIPLIERS: [f64; 4] = [2.5, 3.0, 3.5, 4.0];
const MIN_BARS_BEFORE_CHANDELIER: usize = 10;
const TAKER_FEE: f64 = 0.001;
const WARMUP: usize = 200;
const MIN_TRADES: usize = 3;

const UNIVERSES: &[(&str, &[&str])] = &[
    (
        "Base5",
        &[
            "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
        ],
    ),
    (
        "NoDOGE",
        &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"],
    ),
    (
        "Legacy4",
        &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"],
    ),
    (
        "Legacy5BNB",
        &[
            "BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT", "EOSUSDT",
        ],
    ),
    (
        "OldGuardNoBNB",
        &[
            "BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT",
        ],
    ),
    (
        "LargeCaps5",
        &[
            "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "BNBUSDT", "ADAUSDT",
        ],
    ),
    ("Legacy3", &["BTCUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"]),
    (
        "LowVolume5",
        &["XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT", "ADAUSDT"],
    ),
    (
        "OldGuard4",
        &["BTCUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"],
    ),
];

// ── Math helpers ──────────────────────────────────────────────────────────────

fn calc_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 5 {
        return 0.0;
    }
    let mean = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let var = daily_rets
        .iter()
        .map(|r| {
            let x = r - mean;
            x * x
        })
        .sum::<f64>()
        / daily_rets.len().max(1) as f64;
    let std = var.sqrt();
    if std < 1e-9 {
        return 0.0;
    }
    mean * 365.0 / (std * (365.0_f64).sqrt())
}

fn true_range(high: f64, low: f64, prev_close: f64) -> f64 {
    let t1 = (high - low).abs();
    let t2 = (high - prev_close).abs();
    let t3 = (low - prev_close).abs();
    t1.max(t2).max(t3)
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period {
        return 0.0;
    }
    let mut tr_sum = 0.0_f64;
    for i in (idx.saturating_sub(period - 1))..=idx {
        let pc = if i > 0 { close[i - 1] } else { close[0] };
        tr_sum += true_range(high[i], low[i], pc);
    }
    tr_sum / period as f64
}

// ── A/D Momentum signal ───────────────────────────────────────────────────────

fn ad_signal(
    close: &[f64],
    high: &[f64],
    low: &[f64],
    vol: &[f64],
    period: usize,
    train_end: usize,
    bar: usize,
) -> bool {
    let warm = WARMUP.max(period * 2);
    if bar < warm {
        return false;
    }

    let af = 2.0 / (period as f64 + 1.0);
    let as_ = 2.0 / (period as f64 * 2.0 + 1.0);

    // A/D value at bar using EMA from warmup
    let mut ema1 = 0.0_f64;
    let mut ema2 = 0.0_f64;
    for j in warm..=bar {
        let r = high[j] - low[j];
        let m = if r > 1e-9 {
            ((close[j] - low[j]) - (high[j] - close[j])) / r
        } else {
            0.0
        };
        ema1 = af * m * vol[j] + (1.0 - af) * ema1;
        ema2 = as_ * m * vol[j] + (1.0 - as_) * ema2;
    }
    let ad_now = ema1 - ema2;

    // Train mean: EMA accumulated from warmup to train_end
    let mut t_ema1 = 0.0_f64;
    let mut t_ema2 = 0.0_f64;
    let mut t_sum = 0.0_f64;
    let mut t_cnt = 0usize;
    for j in warm..=train_end {
        let r = high[j] - low[j];
        let m = if r > 1e-9 {
            ((close[j] - low[j]) - (high[j] - close[j])) / r
        } else {
            0.0
        };
        t_ema1 = af * m * vol[j] + (1.0 - af) * t_ema1;
        t_ema2 = as_ * m * vol[j] + (1.0 - as_) * t_ema2;
        t_sum += t_ema1 - t_ema2;
        t_cnt += 1;
    }
    let ad_mean = if t_cnt > 0 { t_sum / t_cnt as f64 } else { 0.0 };

    ad_now - ad_mean > 0.0
}

// ── Data ─────────────────────────────────────────────────────────────────────

struct SymData {
    close: Vec<f64>,
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

struct Trade {
    entry: usize,
    exit: usize,
    net_ret: f64,
    reason: &'static str,
}

struct WfRow {
    universe: String,
    window: usize,
    mode: String,
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    avg_bars: f64,
    chand_exits: usize,
    pass: bool,
}

// ── Trade generation ──────────────────────────────────────────────────────────

fn trades_fixed(sd: &SymData, train_end: usize, t0: usize, t1: usize) -> Vec<Trade> {
    let n = sd.close.len();
    let mut out = Vec::new();
    let mut bar = t0;
    while bar + 1 + HOLD_FIXED < t1.min(n) {
        if ad_signal(
            &sd.close, &sd.high, &sd.low, &sd.vol, AD_PERIOD, train_end, bar,
        ) {
            let eidx = (bar + 1).min(n - 1);
            let xidx = (bar + 1 + HOLD_FIXED).min(t1.min(n) - 1);
            let ep = sd.open[eidx];
            let xp = sd.open[xidx.max(eidx)];
            if ep > 0.0 && xp > 0.0 {
                let gross = xp / ep - 1.0;
                out.push(Trade {
                    entry: eidx,
                    exit: xidx,
                    net_ret: gross - 2.0 * TAKER_FEE,
                    reason: "fixed",
                });
                bar = xidx;
                continue;
            }
        }
        bar += 1;
    }
    out
}

fn trades_chandelier(
    sd: &SymData,
    train_end: usize,
    t0: usize,
    t1: usize,
    mult: f64,
) -> Vec<Trade> {
    let n = sd.close.len();
    let mut out = Vec::new();
    let mut bar = t0;
    while bar + 1 < t1.min(n) {
        if ad_signal(
            &sd.close, &sd.high, &sd.low, &sd.vol, AD_PERIOD, train_end, bar,
        ) {
            let eidx = (bar + 1).min(n - 1);
            let ep = sd.open[eidx];
            if ep <= 0.0 {
                bar += 1;
                continue;
            }

            let fixed_exit = (bar + 1 + HOLD_FIXED).min(t1.min(n) - 1);
            let mut xidx = fixed_exit;
            let mut reason = "fixed";
            let trail_start = (eidx + MIN_BARS_BEFORE_CHANDELIER).min(fixed_exit);
            let mut hh = sd.high[eidx];

            for j in (eidx + 1)..fixed_exit {
                hh = hh.max(sd.high[j]);
                let atr_j = atr_at(&sd.high, &sd.low, &sd.close, CHANDELIER_PERIOD, j);
                let ch_line = hh - mult * atr_j;
                if j >= trail_start && atr_j > 0.0 && ch_line > 0.0 && sd.close[j] < ch_line {
                    xidx = (j + 1).min(t1.min(n) - 1);
                    reason = "chandelier";
                    break;
                }
            }

            let xp = sd.open[xidx.max(eidx)];
            if xp > 0.0 {
                let gross = xp / ep - 1.0;
                out.push(Trade {
                    entry: eidx,
                    exit: xidx,
                    net_ret: gross - 2.0 * TAKER_FEE,
                    reason,
                });
                bar = xidx;
                continue;
            }
        }
        bar += 1;
    }
    out
}

// ── Equity / metrics ──────────────────────────────────────────────────────────

struct Metrics {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    avg_bars: f64,
    chand_exits: usize,
}

fn compute_metrics(trades: &[Trade], t0: usize, t1: usize) -> Metrics {
    let span = t1 - t0;
    let mut eq = vec![1.0_f64; span];
    let mut wins = 0usize;
    let mut chand_exits = 0usize;
    let mut total_bars = 0usize;

    // Daily returns for Sharpe and Equity
    let mut daily_rets = vec![0.0_f64; span];
    for t in trades {
        if t.net_ret > 0.0 {
            wins += 1;
        }
        if t.reason == "chandelier" {
            chand_exits += 1;
        }
        total_bars += t.exit - t.entry;

        let daily = t.net_ret / (t.exit - t.entry).max(1) as f64;
        let e_start = t.entry.max(t0);
        let e_end = t.exit.min(t1);
        for b in e_start..e_end {
            daily_rets[b - t0] += daily; // Overlapping trades add their daily rets
        }
    }

    // Build equity curve from aggregate daily rets
    for i in 0..span {
        if i == 0 {
            eq[i] = 1.0 + daily_rets[i];
        } else {
            eq[i] = eq[i - 1] * (1.0 + daily_rets[i]);
        }
    }

    let sharpe = calc_sharpe(&daily_rets);

    // Max DD
    let mut peak = 1.0_f64;
    let mut max_dd = 0.0_f64;
    for &e in &eq {
        peak = peak.max(e);
        let dd = (1.0 - e / peak) * 100.0;
        max_dd = max_dd.max(dd);
    }

    let final_ret = (eq.last().copied().unwrap_or(1.0) - 1.0) * 100.0;
    let wr = if !trades.is_empty() {
        wins as f64 / trades.len() as f64 * 100.0
    } else {
        0.0
    };
    let ab = if !trades.is_empty() {
        total_bars as f64 / trades.len() as f64
    } else {
        0.0
    };

    Metrics {
        ret: final_ret,
        sharpe,
        max_dd,
        trades: trades.len(),
        win_rate: wr,
        avg_bars: ab,
        chand_exits,
    }
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    let mut rows: Vec<WfRow> = Vec::new();

    // Load all data
    let loader = DataLoader::new(None, None);
    let all_syms: std::collections::HashSet<String> = UNIVERSES
        .iter()
        .flat_map(|(_, s)| s.iter().map(|&x| x.to_string()))
        .collect();
    let mut cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        if let Ok(df) = loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            let n = df.height();
            min_len = min_len.min(n);
            cache.insert(sym.clone(), df);
        }
    }
    let n = min_len.min(2800);

    macro_rules! cv {
        ($name:expr, $df:expr) => {{
            let c = $df.column($name)?.f64()?;
            c.into_iter()
                .filter_map(|x| x)
                .take(n)
                .collect::<Vec<f64>>()
        }};
    }

    let mut sdm: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        if let Some(df) = cache.get(sym) {
            sdm.insert(
                sym.clone(),
                SymData {
                    close: cv!("close", df),
                    open: cv!("open", df),
                    high: cv!("high", df),
                    low: cv!("low", df),
                    vol: cv!("volume", df),
                },
            );
        }
    }
    println!("Loaded {} symbols, {} bars\n", sdm.len(), n);

    // ── Debug: check A/D signal for BTC in first universe ────────────────────
    if let Some(btc) = sdm.get("BTCUSDT") {
        let train_end = WARMUP + TRAIN_BARS - 1; // 200 + 252 - 1 = 451
        let mut fired = 0usize;
        for bar in (WARMUP + 1)..=train_end.min(btc.close.len() - 1) {
            if ad_signal(
                &btc.close, &btc.high, &btc.low, &btc.vol, AD_PERIOD, train_end, bar,
            ) {
                fired += 1;
            }
        }
        eprintln!(
            "DEBUG: BTC A/D signal fires {fired} times in train period [{}..{}]",
            WARMUP, train_end
        );
    }

    for (uname, symbols) in UNIVERSES {
        println!("=== Universe: {uname} ===");
        let present = symbols.iter().all(|&s| sdm.contains_key(s));
        if !present {
            println!("  Skipping: missing data");
            continue;
        }
        if n < WARMUP + TRAIN_BARS + TEST_BARS + 1 {
            println!("  Skipping: insufficient bars");
            continue;
        }

        for w in 0..4 {
            let test_offset = w * TEST_BARS;
            let train_end = WARMUP + test_offset + TRAIN_BARS - 1;
            let ts = train_end + 1;
            let te = (ts + TEST_BARS).min(n);
            if te - ts < 50 {
                break;
            }

            println!(
                "  Window {}: train=[{}..{}], test=[{}..{}]",
                w + 1,
                WARMUP,
                train_end,
                ts,
                te
            );

            // Aggregate trades across symbols
            let fixed_all: Vec<Trade> = symbols
                .iter()
                .filter_map(|&s| sdm.get(s).map(|sd| trades_fixed(sd, train_end, ts, te)))
                .flatten()
                .collect();

            let m_fixed = compute_metrics(&fixed_all, ts, te);
            let pass_fixed = m_fixed.trades >= MIN_TRADES && m_fixed.ret > 0.0;
            println!("    Fixed54: ret={:.1}%, Sharpe={:.2}, DD={:.1}%, trades={}, WR={:.0}%, avg_bars={:.0}",
                m_fixed.ret, m_fixed.sharpe, m_fixed.max_dd, m_fixed.trades, m_fixed.win_rate, m_fixed.avg_bars);
            rows.push(WfRow {
                universe: uname.to_string(),
                window: w + 1,
                mode: "Fixed54".to_string(),
                ret: m_fixed.ret,
                sharpe: m_fixed.sharpe,
                max_dd: m_fixed.max_dd,
                trades: m_fixed.trades,
                win_rate: m_fixed.win_rate,
                avg_bars: m_fixed.avg_bars,
                chand_exits: 0,
                pass: pass_fixed,
            });

            for &mult in &CHANDELIER_MULTIPLIERS {
                let ch_all: Vec<Trade> = symbols
                    .iter()
                    .filter_map(|&s| {
                        sdm.get(s)
                            .map(|sd| trades_chandelier(sd, train_end, ts, te, mult))
                    })
                    .flatten()
                    .collect();
                let m = compute_metrics(&ch_all, ts, te);
                let pass = m.trades >= MIN_TRADES && m.ret > 0.0;
                let beat = m.sharpe > m_fixed.sharpe;
                println!("    Chand({:.1}): ret={:.1}%, Sharpe={:.2}, DD={:.1}%, trades={}, WR={:.0}%, avg_bars={:.0}, ch_exit={}, beat_fixed={}",
                    mult, m.ret, m.sharpe, m.max_dd, m.trades, m.win_rate, m.avg_bars, m.chand_exits, beat);
                rows.push(WfRow {
                    universe: uname.to_string(),
                    window: w + 1,
                    mode: format!("Chandelier({mult:.1})"),
                    ret: m.ret,
                    sharpe: m.sharpe,
                    max_dd: m.max_dd,
                    trades: m.trades,
                    win_rate: m.win_rate,
                    avg_bars: m.avg_bars,
                    chand_exits: m.chand_exits,
                    pass,
                });
            }
        }
    }

    // ── Summary ─────────────────────────────────────────────────────────────
    println!("\n\n════════════════════════════════════════════════════════════════════");
    println!("            CHANDELIER EXIT vs FIXED54 WALK-FORWARD SUMMARY");
    println!("════════════════════════════════════════════════════════════════════");

    let modes: Vec<&str> = std::iter::once("Fixed54")
        .chain(CHANDELIER_MULTIPLIERS.iter().map(|m| {
            let mut s = String::new();
            s.push_str("Chandelier(");
            s.push_str(&format!("{:.1}", m));
            s.push(')');
            &*Box::leak(s.into_boxed_str())
        }))
        .collect();

    println!(
        "\n{:>16} {:>10} {:>8} {:>8} {:>7} {:>8} {:>9} {:>10} {:>10}",
        "Mode", "AvgRet%", "AvgSh", "AvgDD%", "Trades", "WinRate", "AvgBars", "ChExit", "PassRate"
    );
    println!("{}", "-".repeat(88));

    for mode in &[
        "Fixed54",
        "Chandelier(2.5)",
        "Chandelier(3.0)",
        "Chandelier(3.5)",
        "Chandelier(4.0)",
    ] {
        let mr: Vec<_> = rows.iter().filter(|r| r.mode == *mode).collect();
        if mr.is_empty() {
            continue;
        }
        let avg_ret = mr.iter().map(|r| r.ret).sum::<f64>() / mr.len() as f64;
        let avg_sh = mr.iter().map(|r| r.sharpe).sum::<f64>() / mr.len() as f64;
        let avg_dd = mr.iter().map(|r| r.max_dd).sum::<f64>() / mr.len() as f64;
        let tot_trades = mr.iter().map(|r| r.trades).sum::<usize>();
        let avg_wr = mr.iter().map(|r| r.win_rate).sum::<f64>() / mr.len() as f64;
        let avg_bars = mr.iter().map(|r| r.avg_bars).sum::<f64>() / mr.len() as f64;
        let chand_tot = mr.iter().map(|r| r.chand_exits).sum::<usize>();
        let pass_pct = mr.iter().filter(|r| r.pass).count() as f64 / mr.len() as f64 * 100.0;
        println!(
            "{:>16} {:>10.1} {:>8.2} {:>8.1} {:>7} {:>8.0}% {:>9.0} {:>10} {:>10.0}%",
            mode, avg_ret, avg_sh, avg_dd, tot_trades, avg_wr, avg_bars, chand_tot, pass_pct
        );
    }

    // Per-universe best
    println!("\n\nPer-Universe Winner (highest avg OOS Sharpe):");
    for (uname, _symbols) in UNIVERSES {
        let ur: Vec<_> = rows.iter().filter(|r| &r.universe == uname).collect();
        if ur.is_empty() {
            continue;
        }
        let mut best_mode = "Fixed54";
        let mut best_avg = -999.0_f64;
        for mode in &[
            "Fixed54",
            "Chandelier(2.5)",
            "Chandelier(3.0)",
            "Chandelier(3.5)",
            "Chandelier(4.0)",
        ] {
            let vals: Vec<_> = ur
                .iter()
                .filter(|r| r.mode == *mode)
                .map(|r| r.sharpe)
                .collect();
            if !vals.is_empty() {
                let avg = vals.iter().sum::<f64>() / vals.len() as f64;
                if avg > best_avg {
                    best_avg = avg;
                    best_mode = mode;
                }
            }
        }
        let fixed_avg = ur
            .iter()
            .filter(|r| r.mode == "Fixed54")
            .map(|r| r.sharpe)
            .fold(0.0_f64, |s, x| s + x)
            / ur.iter().filter(|r| r.mode == "Fixed54").count().max(1) as f64;
        let beat = best_avg > fixed_avg;
        println!("  {uname}: {best_mode} (avg Sharpe={best_avg:.2}, beat_fixed={beat})");
    }

    // Write CSV
    std::fs::create_dir_all("snapshots")?;
    let mut csvf = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open("snapshots/chandelier_exit_wf.csv")?;
    writeln!(csvf, "universe,window,mode,ret_pct,sharpe,max_dd_pct,trades,win_rate_pct,avg_bars,chand_exits,pass")?;
    for r in &rows {
        writeln!(
            csvf,
            "{},{},{},{:.2},{:.3},{:.2},{},{:.1},{:.1},{},{}",
            r.universe,
            r.window,
            r.mode,
            r.ret,
            r.sharpe,
            r.max_dd,
            r.trades,
            r.win_rate,
            r.avg_bars as usize,
            r.chand_exits,
            r.pass
        )?;
    }
    println!("\nWrote snapshots/chandelier_exit_wf.csv");
    println!("Done in {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}
