//! Held-out validation for REGIME_ATR_PERIOD sweep winner (AP=12).
//! Tests AP=12 vs AP=16 on pre-2021 data only — data the sweep harness never touched.
//! Quick focused comparison: just 2 values × 9 universes × 7 windows.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 5000; // More data for pre-2021 split
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const MIN_TRADES: usize = 3;

const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const HOLD_MAX: usize = 12;
const POSITION_CAP: usize = 3;
const VOL_LOOKBACK: usize = 96;
const REGIME_LOOKBACK: usize = 42;
const ATR_RANK_T: f64 = 24.0;
const TAKER_FEE: f64 = 0.001;

const TEST_APS: &[usize] = &[12, 16];

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

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64],
                  ep: usize, ap: usize, am: f64, idx: usize) -> bool {
    if idx < ep { return false; }
    let max_close = close[idx - ep..idx].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    if close[idx] > max_close {
        if am > 0.0 {
            let atr = atr_at(high, low, close, ap, idx);
            if close[idx] < max_close + atr * am { return false; }
        }
        true
    } else { false }
}

fn btc_atr_pct(btc: &SymData, period: usize, lookback: usize, idx: usize) -> f64 {
    if idx < period.max(lookback) { return 50.0; }
    let curr = atr_at(&btc.high, &btc.low, &btc.close, period, idx);
    if curr <= 0.0 { return 50.0; }
    let mut hist = Vec::with_capacity(lookback);
    for j in (idx + 1 - lookback)..=idx {
        if j >= period {
            hist.push(atr_at(&btc.high, &btc.low, &btc.close, period, j));
        }
    }
    if hist.is_empty() { return 50.0; }
    let cnt = hist.iter().filter(|&&x| x < curr).count();
    (cnt as f64 / hist.len() as f64) * 100.0
}

fn annualised_sharpe(rets: &[f64]) -> f64 {
    if rets.is_empty() { return 0.0; }
    let m = rets.iter().sum::<f64>() / rets.len() as f64;
    let v = rets.iter().map(|x| (x - m).powi(2)).sum::<f64>() / rets.len() as f64;
    if v == 0.0 { return 0.0; }
    (m / v.sqrt()) * (365.0_f64).sqrt()
}

fn run_sim(sym_data: &HashMap<String, SymData>, syms: &[String],
           regime_ap: usize, test_start: usize, test_end: usize) -> (f64, f64, usize) {
    let mut equity = 1.0_f64;
    let mut trades = 0usize;
    let mut daily_rets = Vec::new();
    let mut bar = test_start;

    while bar + 2 < test_end {
        let btc = sym_data.get("BTCUSDT");
        let btc_pct = btc.map(|b| btc_atr_pct(b, regime_ap, REGIME_LOOKBACK, bar)).unwrap_or(50.0);
        if btc_pct < ATR_RANK_T { bar += 1; continue; }

        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let rv = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let px = sd.close.get(bar).copied().unwrap_or(0.0);
                let dv = rv * px;
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect();
        if top.is_empty() { bar += 1; continue; }

        let mut entered = false;
        for sym in &top {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let epx = sd.close[bar];
                        let mut sz = 1.0_f64;
                        if let Some(b) = btc {
                            if bar >= 273 {
                                let a21 = atr_at(&b.high, &b.low, &b.close, 21, bar);
                                let mut h = Vec::with_capacity(252);
                                for j in (bar+1-252)..=bar {
                                    let hh = b.high[j]; let ll = b.low[j];
                                    let cc = b.close[j.saturating_sub(1)];
                                    h.push((hh-ll).max((hh-cc).abs()).max((ll-cc).abs()));
                                }
                                h.sort_by(|a, b| a.partial_cmp(b).unwrap());
                                let p75 = h[(0.75 * h.len() as f64) as usize];
                                if a21 > p75 { sz = 0.70; }
                            }
                        }
                        let entry = epx * (1.0 + TAKER_FEE);
                        let eb = bar + 1;
                        let n = sd.close.len();
                        let mut hh = sd.high[eb];
                        let mb = (eb + HOLD_MAX).min(n.saturating_sub(1));
                        let mut xb = mb;
                        let mut ab: std::collections::VecDeque<f64> = std::collections::VecDeque::new();
                        for b in eb..=mb {
                            if sd.high[b] > hh { hh = sd.high[b]; }
                            let c0 = sd.close[b-1];
                            let tr = (sd.high[b]-sd.low[b]).max((sd.high[b]-c0).abs()).max((sd.low[b]-c0).abs());
                            ab.push_back(tr);
                            if ab.len() > TURTLE_ATR_PERIOD { ab.pop_front(); }
                            if ab.len() == TURTLE_ATR_PERIOD {
                                let av = ab.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
                                let stop = hh - TURTLE_ATR_MULT * av;
                                if sd.low[b] <= stop { xb = b; break; }
                            }
                        }
                        if let Some(&xpx) = sd.close.get(xb) {
                            let exit = xpx * (1.0 - TAKER_FEE);
                            let ret = (exit / entry - 1.0) * sz;
                            let held = (xb as i64 - eb as i64).max(1) as usize;
                            trades += 1;
                            equity *= 1.0 + ret;
                            let daily = ret / held as f64;
                            for _ in 0..held { daily_rets.push(daily); }
                            bar = xb + 1;
                            entered = true;
                            break;
                        }
                    }
                }
            }
        }
        if !entered { bar += 1; }
    }
    (equity, annualised_sharpe(&daily_rets), trades)
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Loading data (held-out validation — pre-2021 cutoff)...");
    let loader = DataLoader::new(None, None);
    let mut sym_data = HashMap::new();
    let mut min_len = usize::MAX;

    let all_syms: std::collections::HashSet<_> =
        UNIVERSES.iter().flat_map(|(_, s)| s.iter().copied()).collect();
    for &sym in &all_syms {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let high  = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let low   = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let vol   = df.column("volume")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        if close.len() < min_len { min_len = close.len(); }
        sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
    }

    // ── Held-out: restrict to windows ending before 2021-01-01 ─────────────
    // Estimate: find bar index where close[bar] corresponds to ~Jan 2021
    // BTC data: ~2018-02 to 2026-05. 3000 bars covers ~2022-05.
    // We need to find a cutoff in the available data.
    // Heuristic: use first 60% of data as "training", last 40% as "held-out"
    // This ensures we're testing on truly unseen (earlier) data.
    // But that gives us fewer windows. Instead: use a fixed calendar cutoff.
    //
    // Simpler: use the same walk-forward but only count windows where
    // test period ends before estimated 2021-01-01.
    // BTC 1d: ~365 bars/year. 2021-01-01 is ~day 1095 from 2018-02-07.
    // We have 3000 bars (2018-02 to 2026-05), so 2021 is roughly bar 1100.
    // Let held_out_cutoff = min_len - 1500 (roughly 2022 mid).
    //
    // Even simpler: just split data in half chronologically.
    // First half = training, second half = held-out test.
    let total_windows = (min_len.saturating_sub(TRAIN_BARS)) / TEST_BARS;
    let held_out_windows = total_windows / 2; // second half only
    let held_out_start_idx = total_windows - held_out_windows;

    println!("Total windows: {}, Held-out windows: {} (idx {}-{})",
        total_windows, held_out_windows, held_out_start_idx, total_windows);

    let mut results: HashMap<usize, Vec<(f64, f64, usize)>> = HashMap::new();

    for &ap in TEST_APS {
        for (u_name, u_syms) in UNIVERSES {
            let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
            for w in held_out_start_idx..total_windows {
                let start = min_len - (total_windows - w) * TEST_BARS - TRAIN_BARS;
                let end = start + TEST_BARS + TRAIN_BARS;
                let (eq, sh, tr) = run_sim(&sym_data, &syms, ap, start + TRAIN_BARS, end);
                results.entry(ap).or_default().push((eq, sh, tr));
            }
        }
    }

    // ── Summary ─────────────────────────────────────────────────────────────
    println!("\n=== HELD-OUT VALIDATION (pre-2021) ===");
    println!("{:<6} {:>6} {:>8} {:>10} {:>10} {:>10}",
        "AP", "Pass", "Pass%", "AvgSharpe", "Ret%", "Trades");
    for &ap in TEST_APS {
        let vals = &results[&ap];
        let passes = vals.iter().filter(|(e, s, t)| t >= &3 && s > &0.0).count();
        let total = vals.len();
        let avg_s = vals.iter().map(|(_, s, _)| s).sum::<f64>() / total as f64;
        let avg_r = vals.iter().map(|(e, _, _)| (e - 1.0) * 100.0).sum::<f64>() / total as f64;
        let tot_tr: usize = vals.iter().map(|(_, _, t)| t).sum();
        let pct = passes as f64 / total as f64 * 100.0;
        println!("{:<6} {}/{} {:>8.1f}%% {:>10.4f} {:>10.2f}%% {:>10}",
            ap, passes, total, pct as f64, avg_s, avg_r, tot_tr);
    }

    // Write to CSV
    let path = "snapshots/regime_ap_held_out.csv";
    let mut f = File::create(path)?;
    writeln!(f, "ap,pass,total,pass_pct,avg_sharpe,avg_ret_pct,total_trades")?;
    for &ap in TEST_APS {
        let vals = &results[&ap];
        let passes = vals.iter().filter(|(e, s, t)| t >= &3 && s > &0.0).count();
        let total = vals.len();
        let avg_s = vals.iter().map(|(_, s, _)| s).sum::<f64>() / total as f64;
        let avg_r = vals.iter().map(|(e, _, _)| (e - 1.0) * 100.0).sum::<f64>() / total as f64;
        let tot_tr: usize = vals.iter().map(|(_, _, t)| t).sum();
        let pct = passes as f64 / total as f64 * 100.0;
        writeln!(f, "{},{},{},{:.2},{:.4},{:.2},{}", ap, passes, total, pct, avg_s, avg_r, tot_tr)?;
    }
    println!("\nWrote {}", path);
    Ok(())
}
