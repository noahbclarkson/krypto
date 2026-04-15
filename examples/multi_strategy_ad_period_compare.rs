//! Quick comparison: Multi-Strategy Portfolio with A/D p=2 vs p=47
//!
//! Tests whether using A/D p=2 (Sharpe champion) improves the portfolio result
//! compared to A/D p=47 (robustness champion).
//! Only changes the A/D period; Turtle+Chandelier stays the same.

use anyhow::Result;
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use polars::prelude::*;
use std::collections::HashMap;
use std::time::Instant;

const TURTLE_ENTRY: usize = 21;
const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.00;
const TURTLE_CAP: usize = 3;
const TURTLE_HM: usize = 45;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const AD_HOLD: usize = 54;

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",        &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE",       &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
    ("Legacy4",      &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("Legacy3",      &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("LowVolume5",   &["XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT","ADAUSDT"]),
];

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let n = daily_rets.len() as f64;
    let mean: f64 = daily_rets.iter().sum::<f64>() / n;
    let var: f64 = daily_rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
    let sd = var.sqrt();
    if sd == 0.0 { return 0.0; }
    mean / sd * 365.0_f64.sqrt()
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut peak = 0.0_f64;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak { peak = e; }
        let dd = (peak - e) / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

fn compute_ad_line(high: &[f64], low: &[f64], close: &[f64], volume: &[f64]) -> Vec<f64> {
    let n = high.len();
    let mut ad = vec![0.0; n];
    for i in 0..n {
        let range = high[i] - low[i];
        let mf = if range > 1e-9 { ((close[i] - low[i]) - (high[i] - close[i])) / range } else { 0.0 };
        ad[i] = if i == 0 { mf * volume[i] } else { ad[i - 1] + mf * volume[i] };
    }
    ad
}

fn correlation(xs: &[f64], ys: &[f64]) -> f64 {
    let n = xs.len().min(ys.len());
    if n < 5 { return 0.0; }
    let mx: f64 = xs[..n].iter().sum::<f64>() / n as f64;
    let my: f64 = ys[..n].iter().sum::<f64>() / n as f64;
    let mut cov = 0.0; let mut vx = 0.0; let mut vy = 0.0;
    for i in 0..n {
        let dx = xs[i] - mx; let dy = ys[i] - my;
        cov += dx * dy; vx += dx * dx; vy += dy * dy;
    }
    let denom = vx.sqrt() * vy.sqrt();
    if denom == 0.0 { return 0.0; }
    cov / denom
}

struct SymData {
    close: Vec<f64>, open: Vec<f64>, high: Vec<f64>, low: Vec<f64>, volume: Vec<f64>,
    ad_mom_p2: Vec<f64>, ad_mom_p47: Vec<f64>,
}

fn sim_turtle(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize, test_end: usize,
) -> (Vec<f64>, f64, usize) {
    let mut daily_rets = Vec::new();
    let mut equity = 1.0_f64;
    let mut trades = 0usize;
    let mut pos: Option<(String, usize, f64, f64)> = None;
    let mut bar = test_start;

    fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
        if idx < period { return 0.0; }
        let mut trs = Vec::with_capacity(period);
        for i in (idx + 1 - period)..=idx {
            let h = high.get(i).copied().unwrap_or(0.0);
            let l = low.get(i).copied().unwrap_or(0.0);
            let c0 = close.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
            trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
        }
        trs.iter().sum::<f64>() / period as f64
    }

    while bar + 1 < test_end {
        if let Some((ref sym, entry_bar, entry_price, ref mut hh)) = pos {
            if let Some(sd) = sym_data.get(sym) {
                if bar < sd.close.len() {
                    *hh = (*hh).max(sd.high.get(bar).copied().unwrap_or(0.0));
                    let atr_val = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, bar);
                    let trail = *hh - CHAND_MULT * atr_val;
                    if sd.close[bar] < trail || (bar - entry_bar) >= TURTLE_HM || bar >= test_end - 1 {
                        let gross = (sd.close[bar] * (1.0 - TAKER_FEE)) / (entry_price * (1.0 + TAKER_FEE)) - 1.0;
                        equity *= 1.0 + gross;
                        trades += 1;
                        let bh = (bar - entry_bar).max(1);
                        let avg = gross / bh as f64;
                        for _ in 0..bh { daily_rets.push(avg); }
                        pos = None;
                    } else { bar += 1; continue; }
                }
            }
        }
        if pos.is_none() {
            let mut scores: Vec<(&String, f64)> = Vec::new();
            for sym in symbols {
                if let Some(sd) = sym_data.get(sym) {
                    if bar >= sd.close.len() { continue; }
                    let dv = sd.volume.get(bar).copied().unwrap_or(0.0) * sd.close.get(bar).copied().unwrap_or(0.0);
                    scores.push((sym, if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
                }
            }
            scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            let top: Vec<&String> = scores.iter().take(TURTLE_CAP).map(|(s, _)| *s).collect();
            let mut entered = false;
            for sym_ref in &top {
                if let Some(sd) = sym_data.get(sym_ref.as_str()) {
                    if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                        let start = bar + 1 - TURTLE_ENTRY;
                        let mut max_close = f64::NEG_INFINITY;
                        for i in start..bar { max_close = max_close.max(sd.close.get(i).copied().unwrap_or(0.0)); }
                        if sd.close[bar] > max_close {
                            let ep = sd.close[bar];
                            let hh = sd.high.get(bar + 1).copied().unwrap_or(ep);
                            pos = Some(((*sym_ref).clone(), bar, ep, hh));
                            entered = true; break;
                        }
                    }
                }
            }
            if !entered { daily_rets.push(0.0); }
        }
        bar += 1;
    }
    if let Some((sym, entry_bar, ep, _)) = pos {
        if let Some(sd) = sym_data.get(&sym) {
            let eb = (test_end - 1).min(sd.close.len() - 1);
            let gross = (sd.close[eb] * (1.0 - TAKER_FEE)) / (ep * (1.0 + TAKER_FEE)) - 1.0;
            equity *= 1.0 + gross; trades += 1;
            let bh = (eb - entry_bar).max(1);
            for _ in 0..bh { daily_rets.push(gross / bh as f64); }
        }
    }
    (daily_rets, (equity - 1.0) * 100.0, trades)
}

fn sim_ad(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    ad_momentum: &dyn Fn(&SymData, usize) -> f64,
    ad_period: usize,
    test_start: usize, test_end: usize,
) -> (Vec<f64>, f64, usize) {
    let mut daily_rets = Vec::new();
    let mut equity = 1.0_f64;
    let mut trades = 0usize;
    let mut pos: Option<(String, usize, f64)> = None;
    let mut bar = test_start;

    while bar + 1 < test_end {
        if let Some((ref sym, entry_bar, ep)) = pos {
            let bh = bar - entry_bar;
            if bh >= AD_HOLD || bar >= test_end - 1 {
                if let Some(sd) = sym_data.get(sym) {
                    let eb = bar.min(sd.close.len() - 1);
                    let gross = (sd.close[eb] * (1.0 - TAKER_FEE)) / (ep * (1.0 + TAKER_FEE)) - 1.0;
                    equity *= 1.0 + gross; trades += 1;
                    for _ in 0..bh.max(1) { daily_rets.push(gross / bh.max(1) as f64); }
                }
                pos = None;
            } else { bar += 1; continue; }
        }
        if pos.is_none() {
            let idx = bar.saturating_sub(1);
            let mut longs: Vec<(&String, f64)> = Vec::new();
            for sym in symbols {
                if let Some(sd) = sym_data.get(sym) {
                    if idx < ad_period { continue; }
                    let mom = ad_momentum(sd, idx);
                    if mom > 0.0 { longs.push((sym, mom)); }
                }
            }
            longs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            if let Some((best, _)) = longs.first() {
                if let Some(sd) = sym_data.get(best.as_str()) {
                    if bar < sd.open.len() && sd.open[bar] > 0.0 {
                        pos = Some(((*best).clone(), bar, sd.open[bar]));
                    }
                }
            }
            if pos.is_none() { daily_rets.push(0.0); }
        }
        bar += 1;
    }
    if let Some((sym, entry_bar, ep)) = pos {
        if let Some(sd) = sym_data.get(&sym) {
            let eb = (test_end - 1).min(sd.close.len() - 1);
            let gross = (sd.close[eb] * (1.0 - TAKER_FEE)) / (ep * (1.0 + TAKER_FEE)) - 1.0;
            equity *= 1.0 + gross; trades += 1;
            let bh = (eb - entry_bar).max(1);
            for _ in 0..bh { daily_rets.push(gross / bh as f64); }
        }
    }
    (daily_rets, (equity - 1.0) * 100.0, trades)
}

fn combine(t: &[f64], a: &[f64]) -> (Vec<f64>, f64) {
    let n = t.len().max(a.len());
    let mut combined = Vec::with_capacity(n);
    let mut eq = 1.0_f64;
    for i in 0..n {
        let tr = t.get(i).copied().unwrap_or(0.0);
        let ar = a.get(i).copied().unwrap_or(0.0);
        let cr = 0.5 * tr + 0.5 * ar;
        combined.push(cr);
        eq *= 1.0 + cr;
    }
    (combined, (eq - 1.0) * 100.0)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("═══ Multi-Strategy Portfolio: A/D p=2 vs p=47 ═══\n");

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES { for &s in *syms { all_syms.insert(s.to_string()); } }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in &all_syms {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => {
                let with_techs = FeatureEngine::add_technicals(&df, None).unwrap_or_else(|_| df.clone());
                min_len = min_len.min(with_techs.height());
                raw_cache.insert(sym.clone(), with_techs);
            }
            Err(e) => { eprintln!("  WARN: {} load failed: {}", sym, e); }
        }
    }
    let n = min_len.min(2800);

    let mut sym_data: HashMap<String, SymData> = HashMap::new();
    for (sym, df) in &raw_cache {
        let n_min = df.height().min(n);
        macro_rules! cv { ($name:expr) => {{ df.column($name)?.f64()?.into_iter().filter_map(|x|x).take(n_min).collect::<Vec<_>>() }}; }
        let close: Vec<f64> = cv!("close"); let open: Vec<f64> = cv!("open");
        let high: Vec<f64> = cv!("high"); let low: Vec<f64> = cv!("low"); let volume: Vec<f64> = cv!("volume");
        let ad_line = compute_ad_line(&high, &low, &close, &volume);
        let mut ad_mom_p2 = vec![0.0; ad_line.len()];
        let mut ad_mom_p47 = vec![0.0; ad_line.len()];
        for i in 2..ad_line.len() { ad_mom_p2[i] = ad_line[i] - ad_line[i-2]; }
        for i in 47..ad_line.len() { ad_mom_p47[i] = ad_line[i] - ad_line[i-47]; }
        sym_data.insert(sym.clone(), SymData { close, open, high, low, volume, ad_mom_p2, ad_mom_p47 });
    }
    eprintln!("Loaded {} symbols, {} bars\n", sym_data.len(), n);

    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        if !symbols.iter().all(|s| sym_data.contains_key(s)) { continue; }
        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 { continue; }

        eprintln!("══ {} ══", label);
        eprintln!("  W   | T ret    T sh | AD47 ret AD47 sh | AD2 ret  AD2 sh | C47 ret C47 sh | C2 ret  C2 sh | C47→C2");

        let mut t47_pass = 0usize; let mut a47_pass = 0usize; let mut a2_pass = 0usize;
        let mut c47_pass = 0usize; let mut c2_pass = 0usize;
        let mut c47_wins = 0usize; let mut c2_wins = 0usize;

        for wi in 0..total_windows {
            let test_start = TRAIN_BARS + wi * TEST_BARS;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end - test_start < 5 { continue; }

            // Turtle (same for both)
            let (t_daily, t_ret, t_trades) = sim_turtle(&sym_data, &symbols, test_start, test_end);
            let t_sh = annualised_sharpe(&t_daily);
            let t_pass = t_trades >= MIN_TRADES && t_ret > 0.0;

            // A/D p=47
            let (a47_daily, a47_ret, a47_trades) = sim_ad(&sym_data, &symbols,
                &|sd: &SymData, idx: usize| sd.ad_mom_p47.get(idx).copied().unwrap_or(0.0), 47,
                test_start, test_end);
            let a47_sh = annualised_sharpe(&a47_daily);
            let a47_pass_w = a47_trades >= MIN_TRADES && a47_ret > 0.0;

            // A/D p=2
            let (a2_daily, a2_ret, a2_trades) = sim_ad(&sym_data, &symbols,
                &|sd: &SymData, idx: usize| sd.ad_mom_p2.get(idx).copied().unwrap_or(0.0), 2,
                test_start, test_end);
            let a2_sh = annualised_sharpe(&a2_daily);
            let a2_pass_w = a2_trades >= MIN_TRADES && a2_ret > 0.0;

            // Combined p=47
            let (c47_daily, c47_ret) = combine(&t_daily, &a47_daily);
            let c47_sh = annualised_sharpe(&c47_daily);
            let c47_pass_w = c47_ret > 0.0;

            // Combined p=2
            let (c2_daily, c2_ret) = combine(&t_daily, &a2_daily);
            let c2_sh = annualised_sharpe(&c2_daily);
            let c2_pass_w = c2_ret > 0.0;

            if t_pass { t47_pass += 1; }
            if a47_pass_w { a47_pass += 1; }
            if a2_pass_w { a2_pass += 1; }
            if c47_pass_w { c47_pass += 1; }
            if c2_pass_w { c2_pass += 1; }
            if c47_sh > t_sh { c47_wins += 1; }
            if c2_sh > t_sh { c2_wins += 1; }

            let c47_vs_c2 = if c2_sh > c47_sh { "C2↑" } else { "C47↑" };

            eprintln!(
                "  W{:02} | {:>+7.1}% {:>5.2} | {:>+7.1}% {:>6.2} | {:>+7.1}% {:>6.2} | {:>+7.1}% {:>6.2} | {:>+7.1}% {:>6.2} | {}",
                wi,
                t_ret, t_sh,
                a47_ret, a47_sh,
                a2_ret, a2_sh,
                c47_ret, c47_sh,
                c2_ret, c2_sh,
                c47_vs_c2,
            );
        }

        let nw = total_windows;
        eprintln!("  PASS | T:{}/{} | AD47:{}/{} | AD2:{}/{} | C47:{}/{} | C2:{}/{}",
            t47_pass, nw, a47_pass, nw, a2_pass, nw, c47_pass, nw, c2_pass, nw);
        eprintln!("  Combined beats Turtle: C47={}/{} C2={}/{}\n", c47_wins, nw, c2_wins, nw);
    }

    eprintln!("Runtime: {:?}", t0.elapsed());
    Ok(())
}
