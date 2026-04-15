//! Vol-Regime Position Sizing Benchmark — Kira Research 2026-04-01
//!
//! Question: does continuous vol-regime position sizing improve MACD+Regime and
//! Turtle+MACD beyond the fixed-hold baseline under fair OOS evaluation?
//!
//! Background:
//! - Both candidates are highly regime-dependent (2026-03-31 walk-forward)
//! - Earlier StressTilt showed gentle macro overlays help risk shape but not raw growth
//! - Vol-regime sizing uses a continuous multiplier (low_vol→1.5x, high_vol→0.5x)
//!   computed WITHOUT look-ahead: 21-bar realized vol percentile vs 252-bar history
//!
//! Design:
//! - signal at close, next-open entry, fixed 21-bar hold
//! - 0.1% taker each side
//! - top-3 MACD-gap-ranked per-window
//! - 4 quarter windows + 15 CPCV-style resamples
//! - Universes: Base5, NoDOGE, OldGuardNoBNB, LargeCaps5

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::path::Path;

const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 5;
const TURTLE_ENTRY: usize = 20;
const VOL_WINDOW: usize = 21;
const VOL_RANK_WINDOW: usize = 252;
const MIN_VOL_MULT: f64 = 0.5;
const MAX_VOL_MULT: f64 = 1.5;

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    macd: Vec<f64>,
    macd_signal: Vec<f64>,
    sma200: Vec<f64>,
    donch_high: Vec<f64>,
    donch_low: Vec<f64>,
    ret: Vec<f64>,
    realized_vol: Vec<f64>,
    vol_pct_rank: Vec<f64>,
}

impl SymData {
    fn from_df(df: &DataFrame) -> Result<Self> {
        let n = df.height();
        let close_ch = df.column("close")?.f64()?;
        let open_ch = df.column("open")?.f64()?;
        let high_ch = df.column("high")?.f64()?;
        let low_ch = df.column("low")?.f64()?;

        let close: Vec<f64> = (0..n).map(|i| close_ch.get(i).unwrap_or(0.0)).collect();
        let open: Vec<f64> = (0..n).map(|i| open_ch.get(i).unwrap_or(0.0)).collect();
        let high: Vec<f64> = (0..n).map(|i| high_ch.get(i).unwrap_or(0.0)).collect();
        let low: Vec<f64> = (0..n).map(|i| low_ch.get(i).unwrap_or(0.0)).collect();

        // ── Compute MACD (12, 26, 9) from raw close ──────────────────────
        let ema_fast = Self::ema(&close, 12);
        let ema_slow = Self::ema(&close, 26);
        let macd_line: Vec<f64> = ema_fast
            .iter()
            .zip(ema_slow.iter())
            .map(|(f, s)| f - s)
            .collect();
        let macd_signal = Self::ema(&macd_line, 9);
        let macd = macd_line;

        // Per-symbol SMA200
        let mut sma200 = vec![0.0; n];
        for i in 200..n {
            sma200[i] = close[(i - 200)..i].iter().sum::<f64>() / 200.0;
        }

        // Turtle 20-bar Donchian channels
        let mut donch_high = vec![0.0; n];
        let mut donch_low = vec![0.0; n];
        for i in TURTLE_ENTRY..n {
            let hh = high[(i.saturating_sub(TURTLE_ENTRY))..i]
                .iter()
                .fold(f64::MIN, |a, &v| a.max(v));
            let ll = low[(i.saturating_sub(TURTLE_ENTRY))..i]
                .iter()
                .fold(f64::MAX, |a, &v| a.min(v));
            donch_high[i] = hh;
            donch_low[i] = ll;
        }

        // Daily returns
        let mut ret = vec![0.0; n];
        for i in 1..n {
            if close[i - 1] > 0.0 {
                ret[i] = (close[i] - close[i - 1]) / close[i - 1];
            }
        }

        // Realized vol (annualized)
        let mut realized_vol = vec![0.0; n];
        for i in VOL_WINDOW..n {
            let slice = &ret[(i - VOL_WINDOW)..i];
            let mean = slice.iter().sum::<f64>() / VOL_WINDOW as f64;
            let var = slice.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / VOL_WINDOW as f64;
            realized_vol[i] = var.sqrt() * (252.0_f64).sqrt();
        }

        // Vol percentile rank (vs past VOL_RANK_WINDOW)
        let mut vol_pct_rank = vec![0.5; n];
        for i in VOL_RANK_WINDOW..n {
            let vol_now = realized_vol[i];
            let start = i.saturating_sub(VOL_RANK_WINDOW);
            let hist = &realized_vol[start..i];
            let below = hist.iter().filter(|&&v| v < vol_now).count() as f64;
            vol_pct_rank[i] = (below / hist.len() as f64).clamp(0.02, 0.98);
        }

        Ok(Self {
            close,
            open,
            high,
            low,
            macd,
            macd_signal,
            sma200,
            donch_high,
            donch_low,
            ret,
            realized_vol,
            vol_pct_rank,
        })
    }

    fn ema(prices: &[f64], period: usize) -> Vec<f64> {
        let n = prices.len();
        if n < period {
            return vec![0.0; n];
        }
        let alpha = 2.0 / (period as f64 + 1.0);
        let mut ema = vec![0.0; n];
        // Seed with SMA
        let seed = prices[..period].iter().sum::<f64>() / period as f64;
        ema[period - 1] = seed;
        for i in period..n {
            ema[i] = alpha * prices[i] + (1.0 - alpha) * ema[i - 1];
        }
        ema
    }

    fn macd_gap(&self, i: usize) -> f64 {
        if i == 0 {
            return 0.0;
        }
        (self.macd[i] - self.macd_signal[i]).abs()
    }

    /// MACD+Regime signal (uses BTC SMA200)
    fn macd_regime_sig(&self, i: usize, btc_sma200: f64) -> i32 {
        if i < 200 || btc_sma200 <= 0.0 {
            return 0;
        }
        let p = self.close[i];
        let m = self.macd[i];
        let ms = self.macd_signal[i];
        if m > ms && p > btc_sma200 {
            1
        } else if m < ms && p < btc_sma200 {
            -1
        } else {
            0
        }
    }

    /// Turtle+MACD signal
    fn turtle_macd_sig(&self, i: usize) -> i32 {
        if i < TURTLE_ENTRY {
            return 0;
        }
        let p = self.close[i];
        let m = self.macd[i];
        let ms = self.macd_signal[i];
        let hh = self.donch_high[i];
        let ll = self.donch_low[i];
        if m > ms && p > hh {
            1
        } else if m < ms && p < ll {
            -1
        } else {
            0
        }
    }

    fn vol_mult(&self, i: usize) -> f64 {
        let pct = self.vol_pct_rank[i];
        MAX_VOL_MULT - (MAX_VOL_MULT - MIN_VOL_MULT) * pct
    }
}

struct BacktestRec {
    return_pct: f64,
    sharpe: f64,
    max_dd: f64,
    n_trades: usize,
    avg_vol_mult: f64,
    pass: bool,
}

fn run_backtest(
    data: &HashMap<String, SymData>,
    signals: &HashMap<String, Vec<i32>>,
    use_vol_scaling: bool,
    start: usize,
    end: usize,
) -> BacktestRec {
    if end <= start || end - start < HOLD_BARS + 2 {
        return BacktestRec {
            return_pct: 0.0,
            sharpe: 0.0,
            max_dd: 0.0,
            n_trades: 0,
            avg_vol_mult: 1.0,
            pass: false,
        };
    }

    let sym0 = data.keys().next().unwrap();
    let n = data.get(sym0).unwrap().close.len();
    let eff_end = end.min(n);

    let mut equity = 1.0_f64;
    let mut peak = equity;
    let mut max_dd = 0.0_f64;
    let mut trades = 0usize;
    let mut rets: Vec<f64> = Vec::new();
    let mut vol_mults: Vec<f64> = Vec::new();
    let mut pos: Option<(String, usize, f64)> = None;

    let sym_list: Vec<String> = data.keys().cloned().collect();
    let mut bar = start;

    while bar + 1 < eff_end {
        if pos.is_none() {
            // Select top-3 by MACD gap strength at bar-1
            let mut cand: Vec<(String, f64)> = Vec::new();
            for sym in &sym_list {
                let sd = data.get(sym).unwrap();
                let sigs = signals.get(sym).unwrap();
                let sig_bar = bar.saturating_sub(1);
                let sig = *sigs.get(sig_bar).unwrap_or(&0);
                if sig == 0 {
                    continue;
                }
                cand.push((sym.clone(), sd.macd_gap(sig_bar)));
            }
            cand.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            for (sym, _) in cand.into_iter().take(3) {
                let sd = data.get(&sym).unwrap();
                let sigs = signals.get(&sym).unwrap();
                let sig_bar = bar.saturating_sub(1);
                let sig = *sigs.get(sig_bar).unwrap_or(&0);
                if sig == 0 {
                    continue;
                }
                let ep = sd.open[bar];
                if ep > 0.0 {
                    pos = Some((sym, bar, ep));
                    break;
                }
            }
        } else {
            let (sym, entry_bar, entry_px) = pos.clone().unwrap();
            let held = bar - entry_bar;
            if held >= HOLD_BARS {
                let xi = (bar + 1).min(eff_end - 1);
                let sd = data.get(&sym).unwrap();
                let xp = sd.open[xi];
                if entry_px > 0.0 && xp > 0.0 {
                    let gross = (xp / entry_px) - 1.0;
                    let net_no_mult = gross - 2.0 * TAKER_FEE;
                    let mult = if use_vol_scaling {
                        sd.vol_mult(entry_bar)
                    } else {
                        1.0
                    };
                    let net = net_no_mult * mult;
                    rets.push(net);
                    vol_mults.push(mult);
                    trades += 1;
                    equity *= 1.0 + net;
                    peak = peak.max(equity);
                    max_dd = max_dd.max((peak - equity) / peak * 100.0);
                }
                pos = None;
            }
        }
        bar += 1;
    }

    if trades == 0 {
        return BacktestRec {
            return_pct: 0.0,
            sharpe: 0.0,
            max_dd: 0.0,
            n_trades: 0,
            avg_vol_mult: 1.0,
            pass: false,
        };
    }

    let return_pct = (equity - 1.0) * 100.0;
    let avg = rets.iter().sum::<f64>() / trades as f64;
    let var = rets.iter().map(|r| (r - avg).powi(2)).sum::<f64>() / trades as f64;
    let sd = var.sqrt().max(1e-10);
    let sh = (avg / sd) * (252.0_f64).sqrt();
    let avg_mult = vol_mults.iter().sum::<f64>() / vol_mults.len() as f64;
    let pass = return_pct > 0.0 && trades >= MIN_TRADES;

    BacktestRec {
        return_pct,
        sharpe: sh,
        max_dd,
        n_trades: trades,
        avg_vol_mult: avg_mult,
        pass,
    }
}

fn load_data(symbols: &[&str], cache_dir: &Path) -> Result<HashMap<String, SymData>> {
    let mut map = HashMap::new();
    for &sym in symbols {
        let fname = format!(
            "{}/{}_{}.parquet",
            cache_dir.display(),
            sym.to_lowercase(),
            "1d"
        );
        let path = Path::new(&fname);
        if !path.exists() {
            eprintln!("  [WARN] File not found: {}", fname);
            continue;
        }
        match DataLoader::load_parquet(path) {
            Ok(df) => {
                if let Ok(sd) = SymData::from_df(&df) {
                    map.insert(sym.to_string(), sd);
                } else {
                    eprintln!("  [WARN] Failed to parse SymData from {}", sym);
                }
            }
            Err(e) => {
                eprintln!("  [WARN] Failed to load {}: {}", sym, e);
            }
        }
    }
    Ok(map)
}

fn generate_signals(data: &HashMap<String, SymData>, strat: &str) -> HashMap<String, Vec<i32>> {
    let mut signals = HashMap::new();
    let btc_data = data.get("BTCUSDT");
    let btc_sma200 = btc_data.map(|d| d.sma200.clone()).unwrap_or_default();

    for (sym, sd) in data {
        let n = sd.close.len();
        let mut sig = vec![0i32; n];
        for i in 200..n {
            let btc_reg = if i < btc_sma200.len() {
                btc_sma200[i]
            } else {
                0.0
            };
            sig[i] = match strat {
                "MACD+Regime" => sd.macd_regime_sig(i, btc_reg),
                "Turtle+MACD" => sd.turtle_macd_sig(i),
                _ => 0,
            };
        }
        signals.insert(sym.clone(), sig);
    }
    signals
}

fn run_quarter_wf(
    data: &HashMap<String, SymData>,
    signals: &HashMap<String, Vec<i32>>,
    use_vol_scaling: bool,
) -> Vec<BacktestRec> {
    let min_len = data.values().map(|d| d.close.len()).min().unwrap_or(0);
    let n_windows = (min_len - TRAIN_BARS) / TEST_BARS;
    let mut results = Vec::new();

    for wi in 0..n_windows {
        let train_start = wi * TEST_BARS;
        let train_end = train_start + TRAIN_BARS;
        let test_start = train_end;
        let test_end = (test_start + TEST_BARS).min(min_len);

        if test_end - test_start < HOLD_BARS + 5 {
            continue;
        }
        let rec = run_backtest(data, signals, use_vol_scaling, test_start, test_end);
        results.push(rec);
    }
    results
}

fn run_cpcv(
    data: &HashMap<String, SymData>,
    signals: &HashMap<String, Vec<i32>>,
    use_vol_scaling: bool,
) -> Vec<BacktestRec> {
    let min_len = data.values().map(|d| d.close.len()).min().unwrap_or(0);
    let n_bars = min_len;
    let n_blocks = 6;
    let block_size = n_bars / n_blocks;
    let mut results = Vec::new();

    // Fixed seed per rep for reproducibility
    for rep in 0..15 {
        let b1 = (rep * 17 + 3) % n_blocks;
        let b2 = (rep * 31 + 7) % n_blocks;
        let b2 = if b2 == b1 { (b2 + 1) % n_blocks } else { b2 };

        let test_start_1 = b1 * block_size;
        let test_end_1 = ((b1 + 1) * block_size).min(n_bars);
        let test_start_2 = b2 * block_size;
        let test_end_2 = ((b2 + 1) * block_size).min(n_bars);

        // Run block 1
        let r1 = run_backtest(data, signals, use_vol_scaling, test_start_1, test_end_1);
        // Run block 2
        let r2 = run_backtest(data, signals, use_vol_scaling, test_start_2, test_end_2);

        // Combine
        let total_trades = r1.n_trades + r2.n_trades;
        if total_trades == 0 {
            results.push(BacktestRec {
                return_pct: 0.0,
                sharpe: 0.0,
                max_dd: 0.0,
                n_trades: 0,
                avg_vol_mult: 1.0,
                pass: false,
            });
        } else {
            // Combine equity curves via geometric chaining
            let eq1 = 1.0 + r1.return_pct / 100.0;
            let eq2 = 1.0 + r2.return_pct / 100.0;
            let combined_eq = eq1 * eq2;
            let combined_ret = (combined_eq - 1.0) * 100.0;
            let avg_sh = (r1.sharpe + r2.sharpe) / 2.0;
            let max_dd = r1.max_dd.max(r2.max_dd);
            let avg_mult = (r1.avg_vol_mult * r1.n_trades as f64
                + r2.avg_vol_mult * r2.n_trades as f64)
                / total_trades as f64;
            let pass = combined_ret > 0.0 && total_trades >= MIN_TRADES;
            results.push(BacktestRec {
                return_pct: combined_ret,
                sharpe: avg_sh,
                max_dd,
                n_trades: total_trades,
                avg_vol_mult: avg_mult,
                pass,
            });
        }
    }
    results
}

fn universe_symbols(universe: &str) -> Vec<&'static str> {
    match universe {
        "Base5" => vec!["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"],
        "NoDOGE" => vec!["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"],
        "OldGuardNoBNB" => vec![
            "BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT",
        ],
        "LargeCaps5" => vec!["BTCUSDT", "ETHUSDT", "BNBUSDT", "XRPUSDT", "ADAUSDT"],
        _ => vec![],
    }
}

fn period_label(wi: usize) -> &'static str {
    match wi {
        0 => "2017-18 Bull",
        1 => "2018-19 Bear",
        2 => "2019-20 Chop",
        3 => "2020-21 MegaBull",
        4 => "2021-22 War",
        5 => "2022-23 Bear",
        6 => "2023-24 Recov",
        7 => "2024-25",
        _ => "Unknown",
    }
}

fn main() -> Result<()> {
    println!("\n{}", "━".repeat(100));
    println!("  VOL-REGIME POSITION SIZING BENCHMARK — Kira Research 2026-04-01");
    println!("{}", "━".repeat(100));
    println!();
    println!(
        "  Hypothesis: low_vol = increase exposure (up to 1.5x), high_vol = reduce (down to 0.5x)"
    );
    println!("  Vol regime: 21-bar realized vol percentile vs 252-bar rolling history");
    println!("  Same signal, same 21-bar hold, 0.1% taker each side, top-3 MACD-gap book");
    println!();

    let cache_dir = std::path::PathBuf::from("data/cache");
    let universes = ["Base5", "NoDOGE", "OldGuardNoBNB", "LargeCaps5"];
    let strategies = ["MACD+Regime", "Turtle+MACD"];

    let mut summary: HashMap<String, SummaryRec> = HashMap::new();

    for universe in &universes {
        let symbols = universe_symbols(universe);
        let data = match load_data(&symbols, &cache_dir) {
            Ok(d) if !d.is_empty() => d,
            _ => {
                eprintln!("  Failed to load {}", universe);
                continue;
            }
        };
        let min_len = data.values().map(|d| d.close.len()).min().unwrap_or(0);
        let n_windows = (min_len - TRAIN_BARS) / TEST_BARS;

        println!("\n{}", format!("  ══ {} ══", universe));
        println!(
            "{:<14} | {:^20} | {:^20} | {:^15} | {:^15}",
            "Window", "BASE", "VOL-SCALED", "BASE", "VOL-SCALED"
        );
        println!(
            "{:<14} | {:>9} {:>6} {:>6} | {:>9} {:>6} {:>6} | {:^15} | {:^15}",
            "", "Return%", "Sharpe", "DD%", "Return%", "Sharpe", "DD%", "Pass", "Win/Lose"
        );
        println!("{}", "-".repeat(105));

        let mut base_q_rets = Vec::new();
        let mut vol_q_rets = Vec::new();
        let mut base_q_pass = 0usize;
        let mut vol_q_pass = 0usize;
        let mut base_cpcv_pass = 0usize;
        let mut vol_cpcv_pass = 0usize;

        for wi in 0..n_windows.min(7) {
            let train_start = wi * TEST_BARS;
            let train_end = train_start + TRAIN_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(min_len);

            if test_end - test_start < HOLD_BARS + 5 {
                continue;
            }

            for strat in &strategies {
                let signals = generate_signals(&data, strat);

                let base_rec = run_backtest(&data, &signals, false, test_start, test_end);
                let vol_rec = run_backtest(&data, &signals, true, test_start, test_end);

                let win_label = if vol_rec.return_pct > base_rec.return_pct {
                    "VOL ▲"
                } else {
                    "BASE ▼"
                };
                let period = period_label(wi);

                println!("{:<14} | {:>9.1} {:>6.2} {:>6.1}% | {:>9.1} {:>6.2} {:>6.1}% | {:>7} {:>6} | {} {}",
                    format!("W{:02} {}", wi, period),
                    base_rec.return_pct, base_rec.sharpe, -base_rec.max_dd,
                    vol_rec.return_pct,  vol_rec.sharpe,  -vol_rec.max_dd,
                    if base_rec.pass { "PASS" } else { "FAIL" },
                    if vol_rec.pass  { "PASS" } else { "FAIL" },
                    strat, win_label);
            }

            // Aggregate for summary
            let signals_base = generate_signals(&data, "MACD+Regime");
            let signals_tm = generate_signals(&data, "Turtle+MACD");

            let b_mr = run_backtest(&data, &signals_base, false, test_start, test_end);
            let v_mr = run_backtest(&data, &signals_base, true, test_start, test_end);
            let b_tm = run_backtest(&data, &signals_tm, false, test_start, test_end);
            let v_tm = run_backtest(&data, &signals_tm, true, test_start, test_end);

            base_q_rets.push(b_mr.return_pct);
            base_q_rets.push(b_tm.return_pct);
            vol_q_rets.push(v_mr.return_pct);
            vol_q_rets.push(v_tm.return_pct);
            if b_mr.pass || b_tm.pass {
                base_q_pass += 1;
            }
            if v_mr.pass || v_tm.pass {
                vol_q_pass += 1;
            }
        }

        // CPCV resamples
        println!("{}", "-".repeat(105));
        println!(
            "{:<14} | {:^20} | {:^20} | {:^15} | {:^15}",
            "CPCV Rep", "BASE (2 blocks)", "VOL (2 blocks)", "BASE", "VOL"
        );
        println!(
            "{:<14} | {:>9} {:>6} {:>6} | {:>9} {:>6} {:>6} | {:^15} | {:^15}",
            "", "Return%", "Sh", "DD%", "Return%", "Sh", "DD%", "Pass", "Win/Lose"
        );
        println!("{}", "-".repeat(105));

        for rep in 0..15 {
            let n_blocks = 6;
            let block_size = min_len / n_blocks;
            let b1 = (rep * 17 + 3) % n_blocks;
            let mut b2 = (rep * 31 + 7) % n_blocks;
            if b2 == b1 {
                b2 = (b2 + 1) % n_blocks;
            }

            let t1s = b1 * block_size;
            let t1e = ((b1 + 1) * block_size).min(min_len);
            let t2s = b2 * block_size;
            let t2e = ((b2 + 1) * block_size).min(min_len);

            for strat in &strategies {
                let signals = generate_signals(&data, strat);
                let r1 = run_backtest(&data, &signals, false, t1s, t1e);
                let r2 = run_backtest(&data, &signals, false, t2s, t2e);
                let rv1 = run_backtest(&data, &signals, true, t1s, t1e);
                let rv2 = run_backtest(&data, &signals, true, t2s, t2e);

                let b_eq = (1.0 + r1.return_pct / 100.0) * (1.0 + r2.return_pct / 100.0);
                let v_eq = (1.0 + rv1.return_pct / 100.0) * (1.0 + rv2.return_pct / 100.0);
                let b_ret = (b_eq - 1.0) * 100.0;
                let v_ret = (v_eq - 1.0) * 100.0;
                let b_pass = b_ret > 0.0 && r1.n_trades + r2.n_trades >= MIN_TRADES;
                let v_pass = v_ret > 0.0 && rv1.n_trades + rv2.n_trades >= MIN_TRADES;
                let b_sh = (r1.sharpe + r2.sharpe) / 2.0;
                let v_sh = (rv1.sharpe + rv2.sharpe) / 2.0;
                let b_dd = r1.max_dd.max(r2.max_dd);
                let v_dd = rv1.max_dd.max(rv2.max_dd);

                if b_pass {
                    base_cpcv_pass += 1;
                }
                if v_pass {
                    vol_cpcv_pass += 1;
                }

                let win_label = if v_ret > b_ret { "VOL ▲" } else { "BASE ▼" };
                println!("{:<14} | {:>9.1} {:>6.2} {:>6.1}% | {:>9.1} {:>6.2} {:>6.1}% | {:>7} {:>6} | {} {}",
                    format!("R{:02}", rep),
                    b_ret, b_sh, -b_dd,
                    v_ret, v_sh, -v_dd,
                    if b_pass { "PASS" } else { "FAIL" },
                    if v_pass { "PASS" } else { "FAIL" },
                    strat, win_label);
            }
        }

        // Summary for this universe
        let avg_base_q = if !base_q_rets.is_empty() {
            base_q_rets.iter().sum::<f64>() / base_q_rets.len() as f64
        } else {
            0.0
        };
        let avg_vol_q = if !vol_q_rets.is_empty() {
            vol_q_rets.iter().sum::<f64>() / vol_q_rets.len() as f64
        } else {
            0.0
        };
        let vol_improves = avg_vol_q > avg_base_q;

        summary.insert(
            universe.to_string(),
            SummaryRec {
                universe: universe.to_string(),
                base_q_pass,
                vol_q_pass,
                base_cpcv_pass,
                vol_cpcv_pass,
                avg_base_q_ret: avg_base_q,
                avg_vol_q_ret: avg_vol_q,
                vol_improves,
            },
        );

        println!(
            "\n  ▶ Quarter: BASE {}/{} windows | VOL {}/{} windows",
            base_q_pass,
            n_windows.min(7),
            vol_q_pass,
            n_windows.min(7)
        );
        println!(
            "  ▶ CPCV:   BASE {}/15 reps | VOL {}/15 reps",
            base_cpcv_pass, vol_cpcv_pass
        );
        println!(
            "  ▶ Avg quarter return: BASE {:+.1}% | VOL {:+.1}% | → {}",
            avg_base_q,
            avg_vol_q,
            if vol_improves {
                "VOL improves"
            } else {
                "BASE holds"
            }
        );
    }

    // Overall summary
    println!("\n{}", "━".repeat(100));
    println!("  SUMMARY: vol-regime sizing vs baseline");
    println!("{}", "━".repeat(100));
    println!(
        "{:<18} | {:>18} | {:>18} | {:>10}",
        "Universe", "BASE (pass Q/CPCV)", "VOL (pass Q/CPCV)", "VOL Wins?"
    );
    println!("{}", "-".repeat(80));

    for (universe, s) in &summary {
        let vol_wins = if s.vol_improves {
            "✓ VOL"
        } else {
            "✗ BASE"
        };
        println!(
            "{:<18} | {:>9}/{:<8} | {:>9}/{:<8} | {}",
            s.universe,
            s.base_q_pass,
            format!("{}/15", s.base_cpcv_pass),
            s.vol_q_pass,
            format!("{}/15", s.vol_cpcv_pass),
            vol_wins
        );
    }

    let vol_wins_count = summary.values().filter(|s| s.vol_improves).count();
    println!(
        "\n  Vol-regime sizing improves avg quarter return in {}/{} universes",
        vol_wins_count,
        summary.len()
    );
    println!("\n  KEY FINDINGS:");
    println!(
        "  - Vol-regime sizing: continuous multiplier based on 21-bar realized vol percentile"
    );
    println!("  - Low vol (low pct rank) → up to 1.5x exposure | High vol → down to 0.5x");
    println!("  - No look-ahead: percentile rank uses only PAST 252-bar vol history");
    println!("  - Compared against same signal + same 21-bar hold + 0.1% taker");
    println!();

    Ok(())
}

struct SummaryRec {
    universe: String,
    base_q_pass: usize,
    vol_q_pass: usize,
    base_cpcv_pass: usize,
    vol_cpcv_pass: usize,
    avg_base_q_ret: f64,
    avg_vol_q_ret: f64,
    vol_improves: bool,
}
