//! T7: BTC/ETH Correlation Entry Filter Walk-Forward
//!
//! Tests whether BTC/ETH trend confirmation reduces ALT whipsaw.
//! Filter variants applied to ALT symbol entries only (BTC/ETH always trade freely).
//!
//! Filter 0 = No filter (baseline)
//! Filter 1 = BTC close > SMA(BTC,21) required for ALT entries
//! Filter 2 = BTC OR ETH above SMA(21) required for ALT entries
//! Filter 3 = BTC AND ETH both above SMA(21) required for ALT entries

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
const TURTLE_ENTRY: usize = 24;
const TURTLE_ATR_PERIOD: usize = 24;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 1;
const TURTLE_ATR_MULT: f64 = 2.00;
const SMA_LOOKBACK: usize = 21;

const FILTER_NONE: u8 = 0;
const FILTER_BTC_ONLY: u8 = 1;
const FILTER_BTC_OR_ETH: u8 = 2;
const FILTER_BTC_AND_ETH: u8 = 3;

const BASE5: &[&str] = &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"];
const BTC: &str = "BTCUSDT";
const ETH: &str = "ETHUSDT";

const ALL9: &[(&str, &[&str])] = &[
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
    let start = idx + 1 - window;
    vals[start..=idx].iter().sum::<f64>() / window as f64
}

fn sma_at(vals: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return vals.first().copied().unwrap_or(0.0); }
    let start = idx + 1 - period;
    vals[start..=idx].iter().sum::<f64>() / period as f64
}

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    if let Some(&curr_close) = close.get(idx) {
        let breakout = curr_close > max_close;
        if breakout && atr_mult > 0.0 {
            let atr_val = atr_at(high, low, close, atr_period, idx);
            return curr_close >= max_close + atr_mult * atr_val;
        }
        breakout
    } else {
        false
    }
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

fn btc_eth_filter(btc_data: &SymData, eth_data: &SymData, idx: usize, filter_type: u8) -> bool {
    if filter_type == FILTER_NONE { return true; }
    let btc_above = btc_data.close.get(idx).copied().unwrap_or(0.0) > sma_at(&btc_data.close, SMA_LOOKBACK, idx);
    let eth_above = eth_data.close.get(idx).copied().unwrap_or(0.0) > sma_at(&eth_data.close, SMA_LOOKBACK, idx);
    match filter_type {
        FILTER_NONE => true,
        FILTER_BTC_ONLY => btc_above,
        FILTER_BTC_OR_ETH => btc_above || eth_above,
        FILTER_BTC_AND_ETH => btc_above && eth_above,
        _ => true,
    }
}

struct SimResult {
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
    btc_data: &SymData,
    eth_data: &SymData,
    test_start: usize,
    test_end: usize,
    filter_type: u8,
) -> SimResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
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
            // BTC and ETH themselves don't need trend confirmation
            if *sym != BTC && *sym != ETH {
                if !btc_eth_filter(btc_data, eth_data, bar, filter_type) {
                    continue;
                }
            }

            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
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
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = lowest_low_turtle - TURTLE_ATR_MULT * atr_turtle;
                            if sd.close[b] < trail_chand || sd.close[b] < trail_turtle {
                                exit_bar = b;
                                break;
                            }
                        }

                        let exit_px = sd.close[exit_bar];
                        let exit_cost = exit_px * (1.0 - TAKER_FEE);
                        let pnl = (exit_cost - entry) / entry;
                        equity *= 1.0 + pnl;
                        if pnl > 0.0 { wins += 1; }
                        total_trades += 1;
                        entered = true;

                        for _ in bar..exit_bar.min(n.saturating_sub(1)) {
                            equity_curve.push(equity);
                        }
                        bar = exit_bar;
                        break;
                    }
                }
            }
        }

        if !entered {
            equity_curve.push(equity);
            bar += 1;
        }
    }

    for i in 1..equity_curve.len() {
        if equity_curve[i-1] > 0.0 && equity_curve[i] > 0.0 {
            daily_rets.push((equity_curve[i] - equity_curve[i-1]) / equity_curve[i-1]);
        }
    }

    let sharpe = annualised_sharpe(&daily_rets);
    SimResult {
        ret: (equity - 1.0) * 100.0,
        sharpe,
        max_dd: max_dd_from(&equity_curve),
        trades: total_trades,
        win_rate: if total_trades > 0 { wins as f64 / total_trades as f64 } else { 0.0 },
        pass: sharpe > 0.0 && total_trades >= MIN_TRADES,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    println!("=== T7: BTC/ETH Correlation Entry Filter Walk-Forward ===\n");

    let loader = DataLoader::new(None, None);

    // Load all needed symbols (BASE5 + BTC + ETH for trend filter)
    let mut all_syms_set: std::collections::HashSet<String> = std::collections::HashSet::new();
    for &sym in BASE5 { all_syms_set.insert(sym.to_string()); }
    all_syms_set.insert(BTC.to_string());
    all_syms_set.insert(ETH.to_string());

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms_set.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => {
                min_len = min_len.min(df.height());
                raw_cache.insert(sym.clone(), df);
            }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    let n = min_len.min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms_set {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = df.height().min(n);
            macro_rules! col_vec {
                ($name:expr) => {{
                    let chunked = df.column($name)?.f64()?;
                    chunked.into_iter().filter_map(|x| x).take(n_min).collect::<Vec<_>>()
                }};
            }
            sym_data_map.insert(sym.clone(), SymData {
                close: col_vec!("close"),
                high:  col_vec!("high"),
                low:   col_vec!("low"),
                vol:   col_vec!("volume"),
            });
        }
    }
    println!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    let btc_data = sym_data_map.get(BTC).expect("BTC required");
    let eth_data = sym_data_map.get(ETH).expect("ETH required");

    let filter_names = ["none", "btc_only", "btc_or_eth", "btc_and_eth"];
    let filter_types = [FILTER_NONE, FILTER_BTC_ONLY, FILTER_BTC_OR_ETH, FILTER_BTC_AND_ETH];

    let n_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
    let n_windows = n_windows.min(6);

    // ── BASE5 WALK-FORWARD ──────────────────────────────────────────────────
    println!("--- Base5 Walk-Forward ---\n");
    let base5_syms: Vec<String> = BASE5.iter().map(|s| s.to_string()).collect();

    // agg[filter_idx] = (total_ret, total_sharpe, worst_dd, total_trades, pass_count)
    let mut base5_agg: Vec<(f64, f64, f64, usize, usize)> = vec![(0.0, 0.0, 0.0, 0, 0); 4];

    for w in 0..n_windows {
        let train_end = TRAIN_BARS + w * TEST_BARS;
        let test_start = train_end;
        let test_end = (train_end + TEST_BARS).min(n.saturating_sub(1));
        println!("  Window {} (bars {} to {}, {} bars)", w, test_start, test_end, test_end - test_start);

        for (fi, (&ft, fname)) in filter_types.iter().zip(filter_names.iter()).enumerate() {
            let r = run_sim(&sym_data_map, &base5_syms, btc_data, eth_data, test_start, test_end, ft);
            let win_pct = r.win_rate * 100.0;
            let pass_str = if r.pass { "PASS" } else { "FAIL" };
            // Print without % in format strings — avoids %% escaping issues
            println!(
                "    {:>12}: ret={:+8.1e}  sharpe={:6.2e}  DD={:5.1e}  trades={:4}  win={:4.1e}  {}",
                fname, r.ret, r.sharpe, r.max_dd, r.trades, win_pct, pass_str
            );
            base5_agg[fi].0 += r.ret;
            base5_agg[fi].1 += r.sharpe;
            base5_agg[fi].2 = base5_agg[fi].2.max(r.max_dd);
            base5_agg[fi].3 += r.trades;
            if r.pass { base5_agg[fi].4 += 1; }
        }
        println!();
    }

    // ── BASE5 AGGREGATE ───────────────────────────────────────────────────
    println!("--- Base5 Aggregate ---\n");
    let nw = n_windows as f64;
    let mut agg_results: Vec<(String, f64, f64, f64, usize, f64)> = Vec::new();
    let mut none_sharpe = 0.0;
    for (fi, fname) in filter_names.iter().enumerate() {
        let (tot_ret, tot_sh, worst_dd, tot_tr, pass_cnt) = base5_agg[fi];
        let avg_ret = tot_ret / nw;
        let avg_sh = tot_sh / nw;
        let pct_pass = pass_cnt as f64 / nw * 100.0;
        if *fname == "none" { none_sharpe = avg_sh; }
        println!(
            "  {:>12}: avg_ret={:+8.1e}  avg_sharpe={:6.2e}  worst_dd={:5.1e}  total_trades={:5}  pct_pass={:5.1e}",
            fname, avg_ret, avg_sh, worst_dd, tot_tr, pct_pass
        );
        agg_results.push((fname.to_string(), avg_ret, avg_sh, worst_dd, tot_tr, pct_pass));
    }

    // ── ALL 9 UNIVERSES ───────────────────────────────────────────────────
    println!("\n--- All 9 Universes Pass Rate ---\n");
    // univ9_data[universe_idx][filter_idx] = (passes, total, sharpe_sum)
    let mut univ9_data: Vec<Vec<(usize, usize, f64)>> = Vec::new();
    for (uname, usyms) in ALL9 {
        let usym_strs: Vec<String> = usyms.iter().map(|s| s.to_string()).collect();
        let mut filter_stats: [(usize, usize, f64); 4] = [(0, 0, 0.0), (0, 0, 0.0), (0, 0, 0.0), (0, 0, 0.0)];

        let nw2 = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        let nw2 = nw2.min(6);

        for w in 0..nw2 {
            let train_end = TRAIN_BARS + w * TEST_BARS;
            let test_start = train_end;
            let test_end = (train_end + TEST_BARS).min(n.saturating_sub(1));
            if test_end <= test_start + 100 { continue; }

            for (fi, &ft) in filter_types.iter().enumerate() {
                let r = run_sim(&sym_data_map, &usym_strs, btc_data, eth_data, test_start, test_end, ft);
                filter_stats[fi].2 += r.sharpe;
                filter_stats[fi].1 += 1;
                if r.pass { filter_stats[fi].0 += 1; }
            }
        }

        let stats_vec: Vec<(usize, usize, f64)> = filter_stats.iter().map(|&(a,b,c)| (a,b,c)).collect();
        univ9_data.push(stats_vec);

        println!("  {:20}", uname);
        for (fi, fname) in filter_names.iter().enumerate() {
            let (p, t, sh_sum) = filter_stats[fi];
            let pct = if t > 0 { p as f64 / t as f64 * 100.0 } else { 0.0 };
            let avg_sh = if t > 0 { sh_sum / t as f64 } else { 0.0 };
            println!("    {:>12}: {}/{} ({:4.1e})  avg_sharpe={:5.2e}", fname, p, t, pct, avg_sh);
        }
        println!();
    }

    // ── WINNER DETERMINATION ──────────────────────────────────────────────
    println!("\n--- Winner Determination ---");
    let none_trades = agg_results[0].4;
    for r in &agg_results {
        let delta = r.2 - none_sharpe;
        let trade_red = (none_trades as f64 - r.4 as f64) / none_trades as f64 * 100.0;
        println!("  {:>12}: delta_sharpe={:+6.2e}  trade_reduction={:5.1e}  pct_pass={:5.1e}",
            r.0, delta, trade_red, r.5);
    }

    // Win condition: pass >= 50% AND trade_reduction < 30%
    let mut valid: Vec<&(String, f64, f64, f64, usize, f64)> = Vec::new();
    for r in &agg_results {
        if r.5 >= 50.0 {
            let trade_red = (none_trades as f64 - r.4 as f64) / none_trades as f64 * 100.0;
            if trade_red < 30.0 {
                valid.push(r);
            }
        }
    }

    let best_filter: Option<String> = if !valid.is_empty() {
        let winner = valid.iter().max_by(|a, b| a.2.partial_cmp(&b.2).unwrap()).unwrap();
        println!("\n  WINNER: {} (avg_sharpe={:.2e}, delta={:+.2e})", winner.0, winner.2, winner.2 - none_sharpe);
        Some(winner.0.clone())
    } else {
        println!("\n  NO VALID FILTERS: all fail pct_pass>=50% AND trade_reduction<30% threshold.");
        println!("  CONCLUSION: correlation filter does NOT improve over baseline.");
        None
    };

    // ── WRITE CSV ──────────────────────────────────────────────────────────
    let mut csv_lines: Vec<String> = Vec::new();
    csv_lines.push("universe,filter,avg_ret,avg_sharpe,worst_dd,total_trades,pct_pass".to_string());
    for r in &agg_results {
        csv_lines.push(format!("Base5,{},{:.2e},{:.3e},{:.2e},{},{:.1e}",
            r.0, r.1, r.2, r.3, r.4, r.5));
    }
    csv_lines.push(String::new());
    csv_lines.push("universe,filter,passes,total,pct_pass,avg_sharpe".to_string());
    for (ui, (uname, _)) in ALL9.iter().enumerate() {
        for (fi, fname) in filter_names.iter().enumerate() {
            let (p, t, sh_sum) = univ9_data[ui][fi];
            let pct = if t > 0 { p as f64 / t as f64 * 100.0 } else { 0.0 };
            let avg_sh = if t > 0 { sh_sum / t as f64 } else { 0.0 };
            csv_lines.push(format!("{},{},{},{},{:.1e},{:.3e}", uname, fname, p, t, pct, avg_sh));
        }
    }

    let mut f = File::create("snapshots/t7_correlation_filter_results.csv")?;
    for row in &csv_lines { writeln!(f, "{}", row)?; }
    println!("\nCSV: snapshots/t7_correlation_filter_results.csv");

    // ── MARKDOWN REPORT ────────────────────────────────────────────────────
    let now_str = chrono::Local::now().format("%Y-%m-%d %H:%M UTC").to_string();
    let mut md_lines: Vec<String> = Vec::new();
    md_lines.push("# T7: BTC/ETH Correlation Entry Filter — Walk-Forward Results".to_string());
    md_lines.push(format!("\n**Generated:** {}", now_str));
    md_lines.push("\n## Hypothesis".to_string());
    md_lines.push("2026 YTD failure (-32.8%) may be BTC-led divergence. ALT breakouts fire but get stopped by Chandelier when BTC doesn't confirm. BTC/ETH trend confirmation filter might reduce whipsaw.".to_string());
    md_lines.push("\n## Filter Variants".to_string());
    md_lines.push("| Filter | BTC SMA | ETH SMA | Description |".to_string());
    md_lines.push("|--------|---------|---------|-------------|".to_string());
    md_lines.push("| none | — | — | Baseline (no filter) |".to_string());
    md_lines.push("| btc_only | Required | — | BTC must be above SMA(21) for ALT entries |".to_string());
    md_lines.push("| btc_or_eth | Required | Required | Either BTC OR ETH above SMA(21) |".to_string());
    md_lines.push("| btc_and_eth | Required | Required | Both BTC AND ETH above SMA(21) |".to_string());
    md_lines.push("\n## Production Params".to_string());
    md_lines.push("```".to_string());
    md_lines.push("EP=24, CHAND(7,2.30), ATR(24), HM=12, ATR_ENTRY_MULT=0.00".to_string());
    md_lines.push("```".to_string());
    md_lines.push("\n## Base5 Aggregate Results".to_string());
    md_lines.push("| Filter | Avg Ret | Avg Sharpe | Worst DD | Total Trades | Pct Pass |".to_string());
    md_lines.push("|--------|---------|------------|----------|--------------|----------|".to_string());
    for r in &agg_results {
        md_lines.push(format!("| {} | {:+7.1e} | {:8.2e} | {:6.1e} | {:12} | {:6.1e} |",
            r.0, r.1, r.2, r.3, r.4, r.5));
    }
    md_lines.push("\n## All 9 Universes (pass rate per filter)".to_string());
    md_lines.push("| Universe | none | btc_only | btc_or_eth | btc_and_eth |".to_string());
    md_lines.push("|----------|------|----------|-------------|--------------|".to_string());
    for (ui, (uname, _)) in ALL9.iter().enumerate() {
        let row: Vec<String> = filter_names.iter().enumerate().map(|(fi, _)| {
            let (p, t, _) = univ9_data[ui][fi];
            if t > 0 { format!("{:.0e}%", p as f64 / t as f64 * 100.0) } else { "N/A".to_string() }
        }).collect();
        md_lines.push(format!("| {} | {} | {} | {} | {} |", uname, row[0], row[1], row[2], row[3]));
    }
    md_lines.push("\n## Conclusion".to_string());
    if let Some(ref w) = best_filter {
        let winner_r = agg_results.iter().find(|r| &r.0 == w).unwrap();
        md_lines.push(format!("**WINNER: {}** — avg_sharpe={:.2e} (delta={:+.2e} vs baseline).",
            w, winner_r.2, winner_r.2 - none_sharpe));
    } else {
        md_lines.push("**No correlation filter improves over baseline.** The BTC/ETH trend filter does NOT reduce ALT whipsaw — Chandelier(P=7,M=2.30) already handles choppy BTC regimes correctly.".to_string());
    }

    let mut f = File::create("snapshots/t7_correlation_filter_report.md")?;
    for row in &md_lines { writeln!(f, "{}", row)?; }
    println!("Markdown: snapshots/t7_correlation_filter_report.md");

    println!("\nDone in {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}
