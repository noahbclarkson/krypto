//! T82: HEDGE_LOOKBACK extensive hyperopt under CURRENT live Turtle-only config.
//!
//! Background:
//!   HEDGE_LOOKBACK=252 has been the default since the hedge was first coded.
//!   It was NOT systematically optimized:
//!     - T67 confirmed HEDGE_ATR_PCT=0.45 wins (all other values identical)
//!     - T75 confirmed HEDGE_ATR_PERIOD=38 (full P∈[5..=100] step 1 sweep)
//!     - T81 updated HEDGE_SIZE_MULT=0.55
//!   HEDGE_LOOKBACK was tested only sparsely in hedge_overlay_extensive_sweep.rs
//!   at {63, 126, 189, 252, 504} with stale ATR_PERIOD=21 — never under
//!   the current (ATR_PERIOD=38, ATR_PCT=0.45, SIZE_MULT=0.55) config.
//!
//! Question:
//!   Is HEDGE_LOOKBACK=252 the robust optimum under current Turtle-only config?
//!
//! Sweep: HEDGE_LOOKBACK ∈ {21, 42, 63, 84, 105, 126, 147, 168, 189,
//!                            210, 252, 294, 336, 378, 504}
//!        15 values × 9 universes × 7 WF windows = 945 config-windows
//!
//! Output:
//!   snapshots/t82_hedge_lookback_summary.csv
//!   snapshots/t82_hedge_lookback_windows.csv
//!   snapshots/t82_hedge_lookback_equity.csv  (Base5 per-LB equity time-series)
//!   charts/comparison_chart.png  (Python script: plot_t82.py)

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const WARMUP_BARS: usize = 300;
const MIN_TRADES: usize = 3;
const FEE: f64 = 0.0004;

// Current hedge config — fixed (NOT swept here)
const HEDGE_ATR_PERIOD: usize = 38;   // T75 winner
const HEDGE_ATR_PCT: f64 = 0.45;      // T67 winner
const HEDGE_SIZE_MULT: f64 = 0.55;     // T81 winner

// Turtle strategy params (from config.rs)
const TURTLE_EP: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.0;
const ATR_ENTRY_MULT: f64 = 0.00;
const HOLD_MAX: usize = 15;            // T81 winner
const POSITION_CAP: usize = 3;
const REGIME_ATR_PERIOD: usize = 17;
const REGIME_LOOKBACK: usize = 41;
const ATR_RANK_THRESHOLD: f64 = 5.0;
const VOL_LOOKBACK: usize = 92;
const FRESHNESS_COOLDOWN: usize = 0;

const BASELINE_LB: usize = 252;
const LB_VALUES: &[usize] = &[
    21, 42, 63, 84, 105, 126, 147, 168, 189, 210, 252, 294, 336, 378, 504,
];

const SUMMARY_OUT: &str =
    "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t82_hedge_lookback_summary.csv";
const WINDOWS_OUT: &str =
    "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t82_hedge_lookback_windows.csv";
const EQUITY_OUT: &str =
    "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t82_hedge_lookback_equity.csv";

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5", &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"]),
    ("NoDOGE", &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"]),
    ("LargeCaps5", &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"]),
    ("LowVolume5", &["LTCUSDT", "EOSUSDT", "BCHUSDT", "NEOUSDT", "QTUMUSDT"]),
    ("Legacy3", &["BTCUSDT", "XRPUSDT", "LTCUSDT"]),
    ("Legacy4", &["BTCUSDT", "XRPUSDT", "LTCUSDT", "ETHUSDT"]),
    ("HighBeta3", &["SOLUSDT", "DOGEUSDT", "ADAUSDT"]),
    ("MidCaps5", &["AVAXUSDT", "MATICUSDT", "DOTUSDT", "LINKUSDT", "UNIUSDT"]),
    ("All10", &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT","LTCUSDT","EOSUSDT","BCHUSDT","BNBUSDT"]),
];

// ─── Data structures ──────────────────────────────────────────────────────────

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
}

#[derive(Clone)]
struct PosState {
    entry_bar: usize,
    entry_exec: f64,
    size: f64,
    highest_high: f64,
    bars_held: usize,
    atr_buf: VecDeque<f64>,
}

#[derive(Clone)]
struct SimResult {
    final_equity: f64,
    sharpe: f64,
    max_dd_pct: f64,
    trades: usize,
    wins: usize,
    equity_curve: Vec<f64>,
}

// ─── True Range ────────────────────────────────────────────────────────────────

fn tr_at(sd: &SymData, idx: usize) -> f64 {
    let pc = if idx == 0 { sd.close[idx] } else { sd.close[idx - 1] };
    (sd.high[idx] - sd.low[idx])
        .max((sd.high[idx] - pc).abs())
        .max((sd.low[idx] - pc).abs())
}

fn atr_at(buf: &VecDeque<f64>) -> f64 {
    if buf.is_empty() {
        return 0.0;
    }
    buf.iter().sum::<f64>() / buf.len() as f64
}

// ─── Regime ATR percentile (btc_atr_percentile) ────────────────────────────────

fn btc_atr_percentile(btc: &SymData, idx: usize) -> f64 {
    let len = idx + 1;
    if len <= REGIME_ATR_PERIOD.max(REGIME_LOOKBACK) + 1 {
        return 50.0;
    }
    let mut curr_trs = Vec::with_capacity(REGIME_ATR_PERIOD);
    for i in (len - REGIME_ATR_PERIOD)..len {
        curr_trs.push(tr_at(btc, i));
    }
    let curr_atr = curr_trs.iter().sum::<f64>() / REGIME_ATR_PERIOD as f64;
    if curr_atr <= 0.0 {
        return 50.0;
    }
    let curr_pct = curr_atr / btc.close[idx];
    let start = len.saturating_sub(REGIME_LOOKBACK);
    let mut below = 0usize;
    let mut total = 0usize;
    for i in start..idx {
        if btc.close[i] <= 0.0 {
            continue;
        }
        let mut h_trs = Vec::with_capacity(REGIME_ATR_PERIOD);
        for k in (i + 1 - REGIME_ATR_PERIOD)..=i {
            h_trs.push(tr_at(btc, k));
        }
        let hist_atr = h_trs.iter().sum::<f64>() / REGIME_ATR_PERIOD as f64;
        if hist_atr <= 0.0 {
            continue;
        }
        if hist_atr / btc.close[i] < curr_pct {
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

// ─── Hedge active (parameterized by HEDGE_LOOKBACK) ──────────────────────────

fn hedge_active(btc: &SymData, idx: usize, hedge_lb: usize) -> bool {
    let n = idx + 1;
    if HEDGE_ATR_PERIOD == 0 || n < hedge_lb + HEDGE_ATR_PERIOD {
        return false;
    }
    let mut trs = Vec::with_capacity(HEDGE_ATR_PERIOD);
    for i in (n - HEDGE_ATR_PERIOD)..n {
        trs.push(tr_at(btc, i));
    }
    let curr_atr = trs.iter().sum::<f64>() / HEDGE_ATR_PERIOD as f64;
    let mut hist = Vec::with_capacity(hedge_lb);
    for j in 1..=hedge_lb {
        let h_idx = n.saturating_sub(j);
        if h_idx == 0 {
            break;
        }
        hist.push(tr_at(btc, h_idx));
    }
    if hist.is_empty() {
        return false;
    }
    hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct_idx = (HEDGE_ATR_PCT * hist.len() as f64) as usize;
    if let Some(&threshold) = hist.get(pct_idx.min(hist.len() - 1)) {
        curr_atr > threshold
    } else {
        false
    }
}

// ─── Turtle entry signal ─────────────────────────────────────────────────────

fn turtle_entry(sd: &SymData, idx: usize) -> bool {
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
        let mut trs = Vec::with_capacity(TURTLE_ATR_PERIOD);
        for i in (len - TURTLE_ATR_PERIOD)..len {
            trs.push(tr_at(sd, i));
        }
        let atr = trs.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
        if sd.close[idx] < max_close + atr * ATR_ENTRY_MULT {
            return false;
        }
    }
    true
}

// ─── Seed ATR buffer ───────────────────────────────────────────────────────────

fn seed_atr_buf(sd: &SymData, idx: usize) -> VecDeque<f64> {
    let len = idx + 1;
    let avail = len.min(TURTLE_ATR_PERIOD);
    let start = len.saturating_sub(avail);
    let mut buf = VecDeque::with_capacity(TURTLE_ATR_PERIOD);
    for offset in 0..avail {
        let b_idx = start + offset;
        let pc = if offset == 0 {
            sd.close[b_idx]
        } else {
            sd.close[start + offset - 1]
        };
        let tr = (sd.high[b_idx] - sd.low[b_idx])
            .max((sd.high[b_idx] - pc).abs())
            .max((sd.low[b_idx] - pc).abs());
        buf.push_back(tr);
    }
    buf
}

// ─── Mark-to-market equity ───────────────────────────────────────────────────

fn mtm_equity(
    realized: f64,
    positions: &HashMap<String, PosState>,
    data: &HashMap<String, SymData>,
    idx: usize,
) -> f64 {
    let mut open_ret = 0.0;
    for (sym, pos) in positions {
        if let Some(sd) = data.get(sym) {
            if idx < sd.close.len() {
                let liq_exec = sd.close[idx] * (1.0 - FEE);
                open_ret += pos.size * (liq_exec / pos.entry_exec - 1.0);
            }
        }
    }
    realized * (1.0 + open_ret)
}

// ─── Metrics ──────────────────────────────────────────────────────────────────

fn annualised_sharpe(equity: &[f64]) -> f64 {
    if equity.len() < 10 {
        return 0.0;
    }
    let mut rets = Vec::with_capacity(equity.len() - 1);
    for w in equity.windows(2) {
        if w[0] > 0.0 {
            rets.push(w[1] / w[0] - 1.0);
        }
    }
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

fn max_dd_pct(equity: &[f64]) -> f64 {
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

// ─── Simulation ───────────────────────────────────────────────────────────────

fn run_sim(
    data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    hedge_lb: usize,
) -> SimResult {
    let btc = match data.get("BTCUSDT") {
        Some(b) => b,
        None => data.get(&symbols[0]).unwrap(),
    };

    let mut realized = 1.0_f64;
    let mut equity_curve: Vec<f64> = Vec::with_capacity(test_end - test_start);
    let mut positions: HashMap<String, PosState> = HashMap::new();
    let mut last_exit_bar: HashMap<String, usize> = HashMap::new();
    let mut trades = 0usize;
    let mut wins = 0usize;

    for idx in test_start..test_end {
        // ── Process exits: collect actions first to avoid borrow conflicts ──
        let mut to_exit: Vec<String> = Vec::new();
        let mut to_update: Vec<(String, PosState)> = Vec::new();

        for (sym, pos) in &positions {
            if idx > pos.entry_bar {
                let sd = data.get(sym).unwrap();
                let mut new_buf = pos.atr_buf.clone();
                let pc = sd.close[idx];
                let tr = (sd.high[idx] - sd.low[idx])
                    .max((sd.high[idx] - pc).abs())
                    .max((sd.low[idx] - pc).abs());
                new_buf.push_back(tr);
                if new_buf.len() > TURTLE_ATR_PERIOD {
                    new_buf.pop_front();
                }
                let bars_held = pos.bars_held + 1;

                let mut exit = false;
                if bars_held >= HOLD_MAX {
                    exit = true;
                } else if new_buf.len() == TURTLE_ATR_PERIOD {
                    let atr_val = atr_at(&new_buf);
                    let stop = pos.highest_high - TURTLE_ATR_MULT * atr_val;
                    if sd.low[idx] <= stop {
                        exit = true;
                    }
                }

                if exit {
                    let exit_exec = sd.close[idx] * (1.0 - FEE);
                    let trade_ret = exit_exec / pos.entry_exec - 1.0;
                    realized *= 1.0 + pos.size * trade_ret;
                    trades += 1;
                    if trade_ret > 0.0 { wins += 1; }
                    last_exit_bar.insert(sym.clone(), idx + 1);
                    to_exit.push(sym.clone());
                } else {
                    let new_hh = sd.high[idx].max(pos.highest_high);
                    to_update.push((sym.clone(), PosState {
                        entry_bar: pos.entry_bar,
                        entry_exec: pos.entry_exec,
                        size: pos.size,
                        highest_high: new_hh,
                        bars_held,
                        atr_buf: new_buf,
                    }));
                }
            }
        }
        // Apply exit decisions
        for sym in to_exit { positions.remove(&sym); }
        for (sym, pos) in to_update { positions.insert(sym, pos); }

        // ── Process entries ─────────────────────────────────────────────
        for sym in symbols {
            if positions.contains_key(sym) {
                continue;
            }
            let current_positions = positions.len();
            if current_positions >= POSITION_CAP {
                continue;
            }
            if let Some(&last_exit) = last_exit_bar.get(sym) {
                let bars_since = (idx + 1).saturating_sub(last_exit);
                if bars_since < FRESHNESS_COOLDOWN {
                    continue;
                }
            }

            let btc_pct = btc_atr_percentile(btc, idx);
            if btc_pct < ATR_RANK_THRESHOLD {
                continue;
            }
            if let Some(sd) = data.get(sym) {
                if !turtle_entry(sd, idx) {
                    continue;
                }
            } else {
                continue;
            }

            let sd = data.get(sym).unwrap();
            let mut size = 1.0 / POSITION_CAP as f64;
            if hedge_active(btc, idx, hedge_lb) {
                size *= HEDGE_SIZE_MULT;
            }
            let entry_exec = sd.close[idx] * (1.0 + FEE);
            positions.insert(
                sym.clone(),
                PosState {
                    entry_bar: idx,
                    entry_exec,
                    size,
                    highest_high: sd.high[idx],
                    bars_held: 0,
                    atr_buf: seed_atr_buf(sd, idx),
                },
            );
        }

        // ── Mark-to-market ──────────────────────────────────────────────
        equity_curve.push(mtm_equity(realized, &positions, data, idx));
    }

    // ── Liquidate remaining positions ───────────────────────────────────────
    if test_end > test_start {
        let last_idx = test_end - 1;
        for (sym, pos) in positions {
            if let Some(sd) = data.get(&sym) {
                if last_idx < sd.close.len() {
                    let exit_exec = sd.close[last_idx] * (1.0 - FEE);
                    let trade_ret = exit_exec / pos.entry_exec - 1.0;
                    realized *= 1.0 + pos.size * trade_ret;
                    trades += 1;
                    if trade_ret > 0.0 {
                        wins += 1;
                    }
                }
            }
        }
        if let Some(last) = equity_curve.last_mut() {
            *last = realized;
        }
    }

    SimResult {
        final_equity: realized,
        sharpe: annualised_sharpe(&equity_curve),
        max_dd_pct: max_dd_pct(&equity_curve),
        trades,
        wins,
        equity_curve,
    }
}

// ─── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== T82: HEDGE_LOOKBACK Extensive Hyperopt ===");
    println!(
        "Turtle config: EP={}, ATR({}, {:.2}), HM={}, CAP={}, ATR_ENTRY={:.2}",
        TURTLE_EP, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, HOLD_MAX, POSITION_CAP, ATR_ENTRY_MULT
    );
    println!(
        "Regime: AP={}, LB={}, T={:.1}",
        REGIME_ATR_PERIOD, REGIME_LOOKBACK, ATR_RANK_THRESHOLD
    );
    println!(
        "Fixed hedge: ATR_P={}, ATR_PCT={:.2}, SIZE_MULT={:.2}",
        HEDGE_ATR_PERIOD, HEDGE_ATR_PCT, HEDGE_SIZE_MULT
    );
    println!(
        "Sweeping HEDGE_LOOKBACK: {:?}",
        LB_VALUES
    );
    println!(
        "Universes: {} × {} windows × {} LB = {} total runs",
        UNIVERSES.len(),
        7,
        LB_VALUES.len(),
        UNIVERSES.len() * 7 * LB_VALUES.len()
    );

    // ── Load data ──────────────────────────────────────────────────────────
    let loader = DataLoader::new(None, None);
    let mut all_syms: HashSet<String> = HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms {
            all_syms.insert(s.to_string());
        }
    }
    all_syms.insert("BTCUSDT".to_string());

    let mut data: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => {
                let close = df.column("close")?.f64()?.into_no_null_iter().collect();
                let high = df.column("high")?.f64()?.into_no_null_iter().collect();
                let low = df.column("low")?.f64()?.into_no_null_iter().collect();
                data.insert(sym.clone(), SymData { close, high, low });
            }
            Err(e) => {
                eprintln!("  WARNING: {} failed to load: {:?}", sym, e);
            }
        }
    }
    println!("Loaded {} symbols successfully", data.len());

    // ── Per-LB aggregation ────────────────────────────────────────────────
    #[derive(Default)]
    struct Agg {
        pass: usize,
        total: usize,
        trades: usize,
        wins: usize,
        sharpe_sum: f64,
        ret_sum: f64,
        dd_sum: f64,
        base5_pass: usize,
        base5_total: usize,
        base5_sharpe: f64,
    }

    let mut agg: HashMap<usize, Agg> = LB_VALUES.iter().map(|&v| (v, Agg::default())).collect();

    let mut win_csv = File::create(WINDOWS_OUT)?;
    writeln!(win_csv, "universe,window,hedge_lb,final_equity,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass")?;

    for (u_name, u_syms) in UNIVERSES {
        let symbols: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
        let min_len = symbols
            .iter()
            .filter_map(|s| data.get(s).map(|d| d.close.len()))
            .min()
            .unwrap_or(0);
        let n_windows = (min_len.saturating_sub(WARMUP_BARS + TRAIN_BARS)) / TEST_BARS;

        for w in 0..n_windows {
            let start = WARMUP_BARS + w * TEST_BARS;
            let test_start = start + TRAIN_BARS;
            let test_end = (test_start + TEST_BARS).min(min_len);
            if test_end <= test_start + 10 {
                continue;
            }

            for &lb in LB_VALUES {
                let res = run_sim(&data, &symbols, test_start, test_end, lb);
                let ret_pct = (res.final_equity - 1.0) * 100.0;
                let win_rate = if res.trades > 0 {
                    res.wins as f64 / res.trades as f64 * 100.0
                } else {
                    0.0
                };
                let pass = res.trades >= MIN_TRADES && res.sharpe > 0.0 && res.final_equity > 1.0;

                writeln!(win_csv,
                    "{},{},{},{:.8},{:.4},{:.6},{:.4},{},{:.4},{}",
                    u_name, w, lb, res.final_equity, ret_pct, res.sharpe,
                    res.max_dd_pct, res.trades, win_rate, pass
                )?;

                let a = agg.get_mut(&lb).unwrap();
                a.pass += pass as usize;
                a.total += 1;
                a.trades += res.trades;
                a.wins += res.wins;
                a.sharpe_sum += res.sharpe;
                a.ret_sum += ret_pct;
                a.dd_sum += res.max_dd_pct;
                if *u_name == "Base5" {
                    a.base5_total += 1;
                    a.base5_pass += pass as usize;
                    a.base5_sharpe += res.sharpe;
                }
            }
        }
        print!("  {} ({} windows): ", u_name, n_windows);
        for &lb in LB_VALUES {
            print!("LB{lb}={:?} ", "");
        }
        println!();
    }

    // ── Summary CSV ──────────────────────────────────────────────────────
    let mut sum_csv = File::create(SUMMARY_OUT)?;
    writeln!(sum_csv, "hedge_lb,pass_count,total_runs,pass_pct,avg_sharpe,avg_return_pct,avg_max_dd_pct,total_trades,win_rate_pct,base5_pass,base5_total,base5_pass_pct,base5_avg_sharpe")?;

    let mut rows: Vec<(usize, &Agg)> = agg.iter().map(|(k, v)| (*k, v)).collect();
    rows.sort_by(|a, b| {
        b.1.pass
            .partial_cmp(&a.1.pass)
            .unwrap()
            .then_with(|| b.1.sharpe_sum.partial_cmp(&a.1.sharpe_sum).unwrap())
    });

    for (&lb, a) in &rows {
        let n = a.total.max(1) as f64;
        let base5_n = a.base5_total.max(1) as f64;
        let base5_pass_pct = a.base5_pass as f64 / base5_n * 100.0;
        writeln!(
            sum_csv,
            "{},{},{},{:.4},{:.6},{:.4},{:.4},{},{:.4},{},{},{:.4},{:.6}",
            lb,
            a.pass,
            a.total,
            a.pass as f64 / n * 100.0,
            a.sharpe_sum / n,
            a.ret_sum / n,
            a.dd_sum / n,
            a.trades,
            if a.trades > 0 { a.wins as f64 / a.trades as f64 * 100.0 } else { 0.0 },
            a.base5_pass,
            a.base5_total,
            base5_pass_pct,
            a.base5_sharpe / base5_n,
        )?;
    }

    // ── Print summary ───────────────────────────────────────────────────
    println!("\n=== T82 HEDGE_LOOKBACK Sweep Results ===");
    println!("{:<6} | {:>4}/{:>4} | {:>6} | {:>8} | {:>6} | {:>7} | {:>6}",
             "LB", "pass", "total", "pass%", "Sharpe", "return%", "DD%", "trades");
    println!("{}", "-".repeat(70));
    for (lb, a) in &rows {
        let n = a.total.max(1) as f64;
        let sh = a.sharpe_sum / n;
        let ret = a.ret_sum / n;
        let dd = a.dd_sum / n;
        let pr = a.pass as f64 / n * 100.0;
        let flag = if *lb == BASELINE_LB { " [BASE]" } else { "" };
        let bn = agg.get(&BASELINE_LB).map(|b| b.total.max(1) as f64).unwrap_or(1.0);
        let br_sharpe = agg.get(&BASELINE_LB).map(|b| b.sharpe_sum / bn).unwrap_or(0.0);
        let br_pass = agg.get(&BASELINE_LB).map(|b| b.pass as f64 / bn * 100.0).unwrap_or(0.0);
        println!(
            "LB={:<5} | pass {:>3}/{:>3} ({:5.1}%%)  | Sharpe {:>+7.4f} {:} | ret {:>+8.2f}% | DD {:>5.2f}% | trades {:>5} | ΔSharpe {:>+7.4f} Δpass {:>+6.1f}pp",
            lb, a.pass, a.total, pr,
            sh, flag,
            ret,
            dd,
            a.trades,
            sh - br_sharpe,
            pr - br_pass
        );
    }

    // ── Base5 equity export ──────────────────────────────────────────────
    let base5_syms: Vec<String> = UNIVERSES[0].1.iter().map(|&s| s.to_string()).collect();
    let base5_min = base5_syms
        .iter()
        .filter_map(|s| data.get(s).map(|d| d.close.len()))
        .min()
        .unwrap_or(0);
    // Use same warm_start as the first window: WARMUP_BARS (start of train period)
    // Equity covers bars from WARMUP_BARS to base5_min, indexed from 0
    let warm_start = WARMUP_BARS;
    let eq_end = base5_min;

    let mut eq_csv = File::create(EQUITY_OUT)?;
    writeln!(eq_csv, "hedge_lb,step,equity")?;
    for &lb in LB_VALUES {
        let res = run_sim(&data, &base5_syms, warm_start, eq_end, lb);
        // Downsample to every 5th bar to keep file manageable
        let stride = ((eq_end - warm_start) / 300).max(1);
        for (step, eq) in res.equity_curve.iter().enumerate() {
            if step % stride == 0 || step == res.equity_curve.len() - 1 {
                writeln!(eq_csv, "{},{},{:.8}", lb, step, eq)?;
            }
        }
    }

    // ── Winner announcement ─────────────────────────────────────────────
    if let Some((&winner_lb, winner_agg)) = rows.first() {
        let n = winner_agg.total.max(1) as f64;
        let bn = agg.get(&BASELINE_LB).map(|b| b.total.max(1) as f64).unwrap_or(1.0);
        let br = agg.get(&BASELINE_LB);
        println!("\n=== WINNER ===");
        println!("HEDGE_LOOKBACK = {} [baseline: {}]", winner_lb, BASELINE_LB);
        println!("  Global: {}/{} ({:.1}%), Sharpe {:.4f}, ret {:+.2f}%, DD {:.2f}%",
                 winner_agg.pass, winner_agg.total,
                 winner_agg.pass as f64 / n * 100.0,
                 winner_agg.sharpe_sum / n,
                 winner_agg.ret_sum / n,
                 winner_agg.dd_sum / n);
        let base5_n = winner_agg.base5_total.max(1) as f64;
        println!("  Base5: {}/{} ({:.1}%), Sharpe {:.4f}",
                 winner_agg.base5_pass, winner_agg.base5_total,
                 winner_agg.base5_pass as f64 / base5_n * 100.0,
                 winner_agg.base5_sharpe / base5_n);
        if let Some(br) = br {
            let delta_sh = winner_agg.sharpe_sum / n - br.sharpe_sum / bn;
            let delta_pr = (winner_agg.pass as f64 / n - br.pass as f64 / bn) * 100.0;
            println!("  vs baseline LB={}: Sharpe Δ={:+.4f}, pass Δ={:+.1f}pp",
                     BASELINE_LB, delta_sh, delta_pr);
        }
    }

    println!("\nOutput:");
    println!("  {SUMMARY_OUT}");
    println!("  {WINDOWS_OUT}");
    println!("  {EQUITY_OUT}");

    Ok(())
}
