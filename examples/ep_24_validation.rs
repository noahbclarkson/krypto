//! EP=24 vs EP=21: Full 9-Universe Validation
//! Tests EP=24 (robust candidate) against baseline EP=21 across all 9 universes.
//! CHAND(P=11, M=2.25) — current production params.

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
const HOLD_MAX: usize = 45;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 11;
const CHAND_MULT: f64 = 2.25;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const VOL_LOOKBACK: usize = 2;

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

const EP_BASELINE: usize = 21;
const EP_CANDIDATE: usize = 24;

struct SymData {
    close: Vec<f64>, high: Vec<f64>, low: Vec<f64>, vol: Vec<f64>,
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

fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    if let Some(&curr_close) = close.get(idx) { curr_close > max_close } else { false }
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
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

fn run_sim(
    sym_data: &HashMap<String, SymData>, symbols: &[String],
    test_start: usize, test_end: usize, ep: usize,
) -> (f64, f64, f64, usize, bool) {
    let mut equity = 1.0_f64; let mut wins = 0usize; let mut total_trades = 0usize;
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
        if top_syms.is_empty() { bar += 1; continue; }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= ep + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, ep, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();
                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut highest_high_turtle = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;
                            highest_high_turtle = highest_high_turtle.max(sd.high[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = highest_high_turtle - TURTLE_ATR_MULT * atr_turtle;
                            if sd.close[b] < trail_chand || sd.close[b] < trail_turtle {
                                exit_bar = b; break;
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
                            for _ in 0..bars_held { daily_rets.push(avg_daily); }
                            bar = exit_bar + 1; entered = true; break;
                        }
                    }
                }
            }
        }
        if !entered { bar += 1; }
    }
    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&[1.0_f64]);
    let pass = total_trades >= MIN_TRADES && ret > 0.0;
    (ret, sharpe, max_dd, total_trades, pass)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== EP=21 vs EP=24 Full 9-Universe Validation ====");

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES { for &s in *syms { all_syms.insert(s.to_string()); } }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    for sym in &all_syms {
        if let Ok(df) = loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            raw_cache.insert(sym.clone(), df);
        }
    }

    let n = raw_cache.values().map(|df| df.height()).min().unwrap_or(2800).min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = df.height().min(n);
            macro_rules! col_vec {
                ($name:expr) => {{
                    let chunked = df.column($name)?.f64()?;
                    chunked.into_iter().filter_map(|x| x).take(n_min).collect::<Vec<_>>()
                }};
            }
            sym_data_map.insert(sym.clone(), SymData {
                close: col_vec!("close"), high: col_vec!("high"),
                low: col_vec!("low"), vol: col_vec!("volume"),
            });
        }
    }
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    let mut csv_lines = vec!["universe,ep,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()];
    let mut global_stats = vec![
        (EP_BASELINE, 0usize, 0usize, 0.0_f64, 0.0_f64),
        (EP_CANDIDATE, 0usize, 0usize, 0.0_f64, 0.0_f64),
    ];

    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        if !symbols.iter().all(|s| sym_data_map.contains_key(s)) { continue; }
        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 { continue; }

        for ep in [EP_BASELINE, EP_CANDIDATE] {
            let mut agg_ret = 0.0_f64; let mut sum_sh = 0.0_f64;
            let mut tot_pass = 0usize; let mut tot_trd = 0usize;
            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { continue; }
                let (ret, sh, _, trades, pass) = run_sim(&sym_data_map, &symbols, test_start, test_end, ep);
                agg_ret += ret; sum_sh += sh; if pass { tot_pass += 1; } tot_trd += trades;
            }
            let nw = total_windows as f64;
            let avg_sh = sum_sh / nw;
            let avg_ret = agg_ret / nw;
            let pp = tot_pass as f64 / nw * 100.0;
            eprintln!("{} EP={} | {}/{} pass {} pct | avg_sh={} | avg_ret={}",
                label, ep, tot_pass, total_windows, pp, avg_sh, avg_ret);
            csv_lines.push(format!("{},{},{:.2},{:.4},,{},,{}",
                label, ep, avg_ret, avg_sh, tot_trd, tot_pass >= total_windows));

            let stat_idx = if ep == EP_BASELINE { 0 } else { 1 };
            let (bep, bp, bt, bsh, bret) = global_stats[stat_idx];
            global_stats[stat_idx] = (bep, bp + tot_pass, bt + total_windows, bsh + avg_sh, bret + avg_ret);
        }
        eprintln!();
    }

    eprintln!("\n═══ GLOBAL SUMMARY ═══");
    for (ep, tot_pass, tot_win, sum_sh, sum_ret) in global_stats {
        let pass_pct = tot_pass as f64 / tot_win as f64 * 100.0;
        let avg_sh = sum_sh / 9.0;
        eprintln!("EP={} | {}/{} pass {} pct | avg_sh={} | sum_avg_ret={}",
            ep, tot_pass, tot_win, pass_pct, avg_sh / (tot_win as f64 / 9.0), sum_ret);
    }

    std::fs::create_dir_all("snapshots")?;
    let mut f = File::create("snapshots/ep_24_validation.csv")?;
    for line in &csv_lines { writeln!(f, "{}", line)?; }
    eprintln!("\nCSV → snapshots/ep_24_validation.csv");
    eprintln!("Done in {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}
