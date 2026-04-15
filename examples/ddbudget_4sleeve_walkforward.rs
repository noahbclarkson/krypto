//! DDBudget 4-Sleeve Walk-Forward Validation
//!
//! PURPOSE: Validate the DDBudget 4-sleeve ensemble under STRICT OOS conditions.
//!
//! The 4 sleeves:
//! 1. A/D Momentum (AD_PERIOD=5, HOLD=54)
//! 2. MACD+Regime (HOLD=21, SMA200 regime filter)
//! 3. SmallByDollarVol (HOLD=21, cross-sectional dollar-volume rank)
//! 4. Cross-Sectional Laggard Short (HOLD=21, short weakest 50% notional, market neutral bias)
//!
//! DDBudget = DD-hard family exposure budgeting:
//!   DD > 30% → 30% exposure | DD > 15% → 60% exposure | else → 100%

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
const HOLD_AD: usize = 54;
const HOLD_OTHER: usize = 21;
const AD_PERIOD: usize = 5;
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

fn ddhard_exposure(dd_pct: f64) -> f64 {
    if dd_pct > 30.0 {
        0.30
    } else if dd_pct > 15.0 {
        0.60
    } else {
        1.0
    }
}

fn calc_max_dd(equity: &[f64]) -> f64 {
    let mut peak = equity[0];
    let mut max_dd = 0.0_f64;
    for &e in equity {
        peak = peak.max(e);
        let dd = (1.0 - e / peak) * 100.0;
        max_dd = max_dd.max(dd);
    }
    max_dd
}

fn ad_signal(
    close: &[f64],
    high: &[f64],
    low: &[f64],
    vol: &[f64],
    period: usize,
    train_end: usize,
    bar: usize,
) -> Option<f64> {
    let warmup = WARMUP.max(period);
    if bar < warmup || bar > train_end {
        return None;
    }

    let alpha1 = 2.0 / (period as f64 + 1.0);
    let alpha2 = 2.0 / (period as f64 * 2.0 + 1.0);

    let mut ema1 = 0.0_f64;
    let mut ema2 = 0.0_f64;
    for j in warmup..=bar.min(train_end) {
        let c = close[j];
        let h = high[j];
        let l = low[j];
        let v = vol[j];
        let range = h - l;
        let mul = if range > 1e-9 {
            ((c - l) - (h - c)) / range
        } else {
            0.0
        };
        ema1 = alpha1 * mul * v + (1.0 - alpha1) * ema1;
        ema2 = alpha2 * mul * v + (1.0 - alpha2) * ema2;
    }
    let ad_now = ema1 - ema2;

    let mut sum = 0.0_f64;
    let mut cnt = 0usize;
    for j in warmup..train_end {
        let c = close[j];
        let h = high[j];
        let l = low[j];
        let v = vol[j];
        let range = h - l;
        let mul = if range > 1e-9 {
            ((c - l) - (h - c)) / range
        } else {
            0.0
        };
        let ea1 = alpha1 * mul * v + (1.0 - alpha1) * 0.0;
        let ea2 = alpha2 * mul * v + (1.0 - alpha2) * 0.0;
        sum += ea1 - ea2;
        cnt += 1;
    }
    let ad_mean = if cnt > 0 { sum / cnt as f64 } else { 0.0 };

    Some(ad_now - ad_mean)
}

fn macd_signal(
    close: &[f64],
    fast: usize,
    slow: usize,
    sig: usize,
    train_end: usize,
    bar: usize,
) -> Option<f64> {
    let warmup = WARMUP.max(slow).max(sig);
    if bar < warmup || bar > train_end {
        return None;
    }

    let ef_alp = 2.0 / (fast as f64 + 1.0);
    let es_alp = 2.0 / (slow as f64 + 1.0);
    let em_alp = 2.0 / (sig as f64 + 1.0);

    let mut ef = 0.0_f64;
    let mut es = 0.0_f64;
    let mut macd_line = 0.0_f64;
    let mut sig_line = 0.0_f64;

    for j in warmup..=bar.min(train_end) {
        ef = ef_alp * close[j] + (1.0 - ef_alp) * ef;
        es = es_alp * close[j] + (1.0 - es_alp) * es;
        let ml = ef - es;
        sig_line = em_alp * ml + (1.0 - em_alp) * sig_line;
        macd_line = ml;
    }

    let sma_start = (bar.saturating_sub(200)).max(warmup);
    let mut sma_sum = 0.0_f64;
    let mut sma_cnt = 0usize;
    for j in sma_start..=bar.min(train_end) {
        sma_sum += close[j];
        sma_cnt += 1;
    }
    let sma200 = if sma_cnt > 0 {
        sma_sum / sma_cnt as f64
    } else {
        close[bar]
    };

    let regime = if close[bar] > sma200 { 1.0 } else { -1.0 };
    let macd_diff = macd_line - sig_line;
    Some(regime * macd_diff)
}

fn smallvol_rank(
    close: &[f64],
    vol: &[f64],
    all_close: &HashMap<&str, &[f64]>,
    all_vol: &HashMap<&str, &[f64]>,
    train_end: usize,
    bar: usize,
) -> Option<f64> {
    let warmup = WARMUP;
    if bar < warmup || bar > train_end {
        return None;
    }

    let my_dv = close[bar] * vol.get(bar).copied().unwrap_or(0.0);
    let mut dvs: Vec<f64> = Vec::new();
    for (sym, c) in all_close {
        if let Some(v) = all_vol.get(sym) {
            dvs.push(c[bar.min(c.len() - 1)] * v.get(bar.min(v.len() - 1)).copied().unwrap_or(0.0));
        }
    }
    if dvs.is_empty() {
        return Some(0.0);
    }
    dvs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pos = dvs
        .iter()
        .position(|&x| x >= my_dv)
        .unwrap_or(dvs.len() - 1);
    Some((dvs.len() - pos) as f64)
}

fn momentum(data: &[f64], period: usize, end: usize) -> f64 {
    if end < period {
        return 0.0;
    }
    let entry = data[end - period];
    if entry > 0.0 {
        (data[end] / entry) - 1.0
    } else {
        0.0
    }
}

struct WfResult {
    window: usize,
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
}

struct SymData {
    close: Vec<f64>,
    vol: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    train_end: usize,
    test_start: usize,
    test_end: usize,
    sleeve_count: usize,
) -> WfResult {
    let all_close: HashMap<&str, &[f64]> = sym_data
        .iter()
        .map(|(k, v)| (k.as_str(), v.close.as_slice()))
        .collect();
    let all_vol: HashMap<&str, &[f64]> = sym_data
        .iter()
        .map(|(k, v)| (k.as_str(), v.vol.as_slice()))
        .collect();

    let mut ad_eq = 1.0_f64;
    let mut mc_eq = 1.0_f64;
    let mut sm_eq = 1.0_f64;
    let mut sh_eq = 1.0_f64;
    let mut ad_pk = 1.0_f64;
    let mut mc_pk = 1.0_f64;
    let mut sm_pk = 1.0_f64;
    let mut sh_pk = 1.0_f64;

    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut daily_rets = Vec::new();
    let mut wins = 0usize;
    let mut total_trades = 0usize;

    let mut bar = test_start;

    while bar + HOLD_AD.max(HOLD_OTHER) + 2 < test_end {
        let mut ad_scores: Vec<(&str, f64)> = Vec::new();
        let mut mc_scores: Vec<(&str, f64)> = Vec::new();
        let mut sm_scores: Vec<(&str, f64)> = Vec::new();
        let mut mom_scores: Vec<(&str, f64)> = Vec::new();

        for sym in symbols {
            let Some(sd) = sym_data.get(sym) else {
                continue;
            };
            if bar >= sd.close.len() {
                continue;
            }

            if let Some(score) = ad_signal(
                &sd.close, &sd.high, &sd.low, &sd.vol, AD_PERIOD, train_end, bar,
            ) {
                ad_scores.push((sym.as_str(), score));
            }
            if let Some(score) = macd_signal(&sd.close, 12, 26, 9, train_end, bar) {
                mc_scores.push((sym.as_str(), score));
            }
            if let Some(score) =
                smallvol_rank(&sd.close, &sd.vol, &all_close, &all_vol, train_end, bar)
            {
                sm_scores.push((sym.as_str(), score));
            }

            let mom = momentum(&sd.close, 20, bar.min(train_end));
            mom_scores.push((sym.as_str(), mom));
        }

        ad_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        mc_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        sm_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        mom_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        let ad_top: Vec<_> = ad_scores.iter().take(2).map(|(s, v)| (*s, *v)).collect();
        let mc_top: Vec<_> = mc_scores.iter().take(2).map(|(s, v)| (*s, *v)).collect();
        let sm_top: Vec<_> = sm_scores.iter().take(2).map(|(s, v)| (*s, *v)).collect();

        let sh_longs: Vec<_> = mom_scores.iter().take(2).map(|(s, _)| *s).collect();
        let sh_shorts: Vec<_> = mom_scores.iter().rev().take(1).map(|(s, _)| *s).collect();

        if ad_top.is_empty() && mc_top.is_empty() && sm_top.is_empty() && sh_longs.is_empty() {
            bar += 1;
            continue;
        }

        let ad_dd = if ad_pk > 0.0 {
            (1.0 - ad_eq / ad_pk) * 100.0
        } else {
            0.0
        };
        let mc_dd = if mc_pk > 0.0 {
            (1.0 - mc_eq / mc_pk) * 100.0
        } else {
            0.0
        };
        let sm_dd = if sm_pk > 0.0 {
            (1.0 - sm_eq / sm_pk) * 100.0
        } else {
            0.0
        };
        let sh_dd = if sh_pk > 0.0 {
            (1.0 - sh_eq / sh_pk) * 100.0
        } else {
            0.0
        };

        let ad_w = ddhard_exposure(ad_dd);
        let mc_w = ddhard_exposure(mc_dd);
        let sm_w = ddhard_exposure(sm_dd);
        let sh_w = ddhard_exposure(sh_dd);

        let tot_w = ad_w
            + (if sleeve_count >= 3 { mc_w + sm_w } else { 0.0 })
            + (if sleeve_count >= 4 { sh_w } else { 0.0 });
        let ad_w = ad_w / tot_w.max(1e-9);
        let mc_w = mc_w / tot_w.max(1e-9);
        let sm_w = sm_w / tot_w.max(1e-9);
        let sh_w = sh_w / tot_w.max(1e-9);

        let mut port_ret = 0.0_f64;

        let mut ad_rets = Vec::new();
        for &(sym, _) in &ad_top {
            let Some(sd) = sym_data.get(sym) else {
                continue;
            };
            let entry = sd.close.get(bar + 1).copied().unwrap_or(0.0);
            let exit = sd.close.get(bar + 1 + HOLD_AD).copied().unwrap_or(0.0);
            if entry > 0.0 && exit > 0.0 {
                let r = (exit / entry - 1.0) - 2.0 * TAKER_FEE;
                ad_rets.push(r);
                ad_pk = ad_pk.max(ad_eq * (1.0 + r));
                total_trades += 1;
                if r > 0.0 {
                    wins += 1;
                }
            }
        }
        if !ad_rets.is_empty() {
            let avg = ad_rets.iter().sum::<f64>() / ad_rets.len() as f64;
            ad_eq *= 1.0 + avg;
            if sleeve_count >= 3 {
                port_ret += ad_w * avg;
            } else {
                port_ret += avg;
            }
        }

        if sleeve_count >= 3 {
            let mut mc_rets = Vec::new();
            for &(sym, _) in &mc_top {
                let Some(sd) = sym_data.get(sym) else {
                    continue;
                };
                let entry = sd.close.get(bar + 1).copied().unwrap_or(0.0);
                let exit = sd.close.get(bar + 1 + HOLD_OTHER).copied().unwrap_or(0.0);
                if entry > 0.0 && exit > 0.0 {
                    let r = (exit / entry - 1.0) - 2.0 * TAKER_FEE;
                    mc_rets.push(r);
                    mc_pk = mc_pk.max(mc_eq * (1.0 + r));
                    total_trades += 1;
                    if r > 0.0 {
                        wins += 1;
                    }
                }
            }
            if !mc_rets.is_empty() {
                let avg = mc_rets.iter().sum::<f64>() / mc_rets.len() as f64;
                mc_eq *= 1.0 + avg;
                port_ret += mc_w * avg;
            }

            let mut sm_rets = Vec::new();
            for &(sym, _) in &sm_top {
                let Some(sd) = sym_data.get(sym) else {
                    continue;
                };
                let entry = sd.close.get(bar + 1).copied().unwrap_or(0.0);
                let exit = sd.close.get(bar + 1 + HOLD_OTHER).copied().unwrap_or(0.0);
                if entry > 0.0 && exit > 0.0 {
                    let r = (exit / entry - 1.0) - 2.0 * TAKER_FEE;
                    sm_rets.push(r);
                    sm_pk = sm_pk.max(sm_eq * (1.0 + r));
                    total_trades += 1;
                    if r > 0.0 {
                        wins += 1;
                    }
                }
            }
            if !sm_rets.is_empty() {
                let avg = sm_rets.iter().sum::<f64>() / sm_rets.len() as f64;
                sm_eq *= 1.0 + avg;
                port_ret += sm_w * avg;
            }

            if sleeve_count >= 4 {
                let mut sh_rets = Vec::new();
                for &sym in &sh_longs {
                    let Some(sd) = sym_data.get(sym) else {
                        continue;
                    };
                    let entry = sd.close.get(bar + 1).copied().unwrap_or(0.0);
                    let exit = sd.close.get(bar + 1 + HOLD_OTHER).copied().unwrap_or(0.0);
                    if entry > 0.0 && exit > 0.0 {
                        let r = (exit / entry - 1.0) - 2.0 * TAKER_FEE;
                        sh_rets.push(r * 0.5); // 50% weight per long
                    }
                }
                for &sym in &sh_shorts {
                    let Some(sd) = sym_data.get(sym) else {
                        continue;
                    };
                    let entry = sd.close.get(bar + 1).copied().unwrap_or(0.0);
                    let exit = sd.close.get(bar + 1 + HOLD_OTHER).copied().unwrap_or(0.0);
                    if entry > 0.0 && exit > 0.0 {
                        let r = (1.0 - exit / entry) - 2.0 * TAKER_FEE;
                        sh_rets.push(r * 0.5); // 50% weight for the short
                    }
                }
                if !sh_rets.is_empty() {
                    let sum: f64 = sh_rets.iter().sum();
                    sh_eq *= 1.0 + sum;
                    sh_pk = sh_pk.max(sh_eq * (1.0 + sum));
                    port_ret += sh_w * sum;
                    total_trades += sh_rets.len();
                    if sum > 0.0 {
                        wins += 1;
                    }
                }
            }
        }

        equity *= 1.0 + port_ret;
        equity_curve.push(equity);
        daily_rets.push(port_ret);

        bar += HOLD_AD + 1;
    }

    let ret = (equity - 1.0) * 100.0;
    let max_dd = calc_max_dd(&equity_curve);
    let sharpe = {
        let test_days = (test_end - test_start) as f64;
        let ann_return = if test_days > 0.0 {
            ret / 100.0 / (test_days / 365.0)
        } else {
            0.0
        };
        if max_dd.abs() > 0.01 {
            ann_return / (max_dd / 100.0)
        } else {
            0.0
        }
    };
    let win_rate = if total_trades > 0 {
        wins as f64 / total_trades as f64 * 100.0
    } else {
        0.0
    };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    WfResult {
        window: 0,
        ret,
        sharpe,
        max_dd,
        trades: total_trades,
        win_rate,
        pass,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    println!("═══ DDBudget 4-Sleeve Walk-Forward Validation ═══\n");
    println!(
        "Config: Train={}b / Test={}b / Non-overlapping / 0.1% taker each side",
        TRAIN_BARS, TEST_BARS
    );

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms {
            all_syms.insert(s.to_string());
        }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;

    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => {
                let n = df.height();
                min_len = min_len.min(n);
                raw_cache.insert(sym.clone(), df);
            }
            Err(e) => {
                eprintln!("  WARNING: {} load failed: {}", sym, e);
            }
        }
    }

    let n = min_len.min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();

    for sym in &all_syms {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = df.height().min(n);
            macro_rules! col_vec {
                ($name:expr) => {{
                    let chunked = df.column($name)?.f64()?;
                    chunked
                        .into_iter()
                        .filter_map(|x| x)
                        .take(n_min)
                        .collect::<Vec<_>>()
                }};
            }
            let close = col_vec!("close");
            let high = col_vec!("high");
            let low = col_vec!("low");
            let vol = col_vec!("volume");
            sym_data_map.insert(
                sym.clone(),
                SymData {
                    close,
                    high,
                    low,
                    vol,
                },
            );
        }
    }

    let mut csv_lines = vec![
        "universe,window,strategy,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass"
            .to_string(),
    ];

    let mut global_4s_pass = 0usize;
    let mut global_4s_total = 0usize;
    let mut global_ad_pass = 0usize;
    let mut global_ad_total = 0usize;

    for &(label, symbols) in UNIVERSES {
        print!("═══ {:<18} ═══ ", label);
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded {
            println!("SKIPPED (missing data)\n");
            continue;
        }

        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 {
            println!("SKIPPED (not enough data)\n");
            continue;
        }
        println!("{} syms, {} windows", symbols.len(), total_windows);

        let mut rows_4s = Vec::new();
        let mut rows_ad = Vec::new();

        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);

            if test_end.saturating_sub(test_start) < HOLD_AD + 5 {
                continue;
            }

            let mut r4s = run_sim(&sym_data_map, &symbols, train_end, test_start, test_end, 4);
            r4s.window = wi;

            let mut r3s = run_sim(&sym_data_map, &symbols, train_end, test_start, test_end, 3);
            r3s.window = wi;

            rows_4s.push(WfResult { ..r4s });
            rows_ad.push(WfResult { ..r3s });

            let mark = |r: &WfResult| {
                if r.pass {
                    "✅"
                } else {
                    "❌"
                }
            };

            print!(
                "  W{:02} | 4Sleeve {:+8.1}% sh={:5.2} DD={:5.1}% {:3}t {:3.0}% {} | 3Sleeve {:+8.1}% sh={:5.2} DD={:5.1}% {:3}t {:3.0}% {}\n",
                wi,
                r4s.ret, r4s.sharpe, r4s.max_dd, r4s.trades, r4s.win_rate, mark(&r4s),
                r3s.ret, r3s.sharpe, r3s.max_dd, r3s.trades, r3s.win_rate, mark(&r3s),
            );

            csv_lines.push(format!(
                "{},{},4sleeve,{:.2},{:.4},{:.2},{},{:.2},{}",
                label,
                wi,
                r4s.ret,
                r4s.sharpe,
                r4s.max_dd,
                r4s.trades,
                r4s.win_rate,
                if r4s.pass { "PASS" } else { "FAIL" }
            ));
        }

        if rows_4s.is_empty() {
            continue;
        }

        let nw = rows_4s.len();
        let p4 = rows_4s.iter().filter(|r| r.pass).count();
        let pa = rows_ad.iter().filter(|r| r.pass).count();
        global_4s_pass += p4;
        global_4s_total += nw;
        global_ad_pass += pa;
        global_ad_total += nw;

        let avg_ret = |r: &[WfResult]| r.iter().map(|x| x.ret).sum::<f64>() / r.len() as f64;
        let avg_sh = |r: &[WfResult]| r.iter().map(|x| x.sharpe).sum::<f64>() / r.len() as f64;
        let worst_dd = |r: &[WfResult]| r.iter().map(|x| x.max_dd).fold(0.0_f64, |a, v| a.max(v));

        println!(
            "  AGG  | 4Sleeve {:+8.1}% sh={:.2} DD={:5.1}% | {}/{} pass | 3Sleeve {:+8.1}% sh={:.2} DD={:5.1}% | {}/{} pass\n",
            avg_ret(&rows_4s), avg_sh(&rows_4s), worst_dd(&rows_4s), p4, nw,
            avg_ret(&rows_ad), avg_sh(&rows_ad), worst_dd(&rows_ad), pa, nw,
        );
    }

    println!("\n═══ GLOBAL SUMMARY ═══");
    println!(
        "  4-Sleeve:  {}/{} windows passed ({:.0}% fail)",
        global_4s_pass,
        global_4s_total,
        if global_4s_total > 0 {
            (global_4s_total - global_4s_pass) as f64 / global_4s_total as f64 * 100.0
        } else {
            0.0
        }
    );
    println!(
        "  A/D-only:   {}/{} windows passed ({:.0}% fail)",
        global_ad_pass,
        global_ad_total,
        if global_ad_total > 0 {
            (global_ad_total - global_ad_pass) as f64 / global_ad_total as f64 * 100.0
        } else {
            0.0
        }
    );
    println!("  Runtime: {:.1}s\n", t0.elapsed().as_secs_f64());

    Ok(())
}
