//! OFI Microstructure Benchmark — Kira Research 2026-04-01
//!
//! Question: does Order Flow Imbalance (OFI) — microstructure from OHLCV — produce a tradeable edge?
//!
//! Two signal families:
//!   MR (mean-reversion):  long lowest OFI cumulative,  short highest OFI
//!   Mom (momentum):       long highest OFI cumulative,  short lowest OFI
//!
//! Execution: signal at close, next-open, 21-bar hold, 0.1% taker, portfolio return per bar

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::path::Path;

const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 10;
const N_POSITIONS: usize = 4; // 2 long + 2 short

// ─── Data ─────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    ofi_cum5: Vec<f64>,
    ofi_cum10: Vec<f64>,
    ofi_cum21: Vec<f64>,
    ofi_diff5: Vec<f64>,
}

impl SymData {
    fn from_df(df: &DataFrame) -> Result<Self> {
        let n = df.height();
        let close = Self::col_f64(df, "close", n);
        let high = Self::col_f64(df, "high", n);
        let low = Self::col_f64(df, "low", n);

        let mut ofi = vec![0.0; n];
        for i in 0..n {
            let r = high[i] - low[i];
            if r > 1e-9 {
                ofi[i] = (close[i] - (high[i] + low[i]) / 2.0) / r;
            }
        }

        Ok(Self {
            close,
            ofi_cum5: Self::rolling_sum(&ofi, 5),
            ofi_cum10: Self::rolling_sum(&ofi, 10),
            ofi_cum21: Self::rolling_sum(&ofi, 21),
            ofi_diff5: Self::rolling_diff(&ofi, 5),
        })
    }

    fn col_f64(df: &DataFrame, name: &str, n: usize) -> Vec<f64> {
        df.column(name)
            .ok()
            .and_then(|c| c.f64().ok())
            .map(|c| (0..n).map(|i| c.get(i).unwrap_or(0.0)).collect())
            .unwrap_or_else(|| vec![0.0; n])
    }

    fn rolling_sum(v: &[f64], w: usize) -> Vec<f64> {
        let n = v.len();
        let mut out = vec![0.0; n];
        for i in w..n {
            out[i] = v[(i - w)..i].iter().sum();
        }
        out
    }

    fn rolling_diff(v: &[f64], lag: usize) -> Vec<f64> {
        let n = v.len();
        let mut out = vec![0.0; n];
        for i in lag..n {
            out[i] = v[i] - v[i - lag];
        }
        out
    }
}

fn universe_symbols(universe: &str) -> Vec<&'static str> {
    match universe {
        "Base5" => vec!["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"],
        "NoDOGE" => vec!["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "BNBUSDT"],
        "OldGuardNoBNB" => vec!["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BCHUSDT"],
        "LargeCaps5" => vec!["BTCUSDT", "ETHUSDT", "BNBUSDT", "XRPUSDT", "LTCUSDT"],
        "Legacy5BNB" => vec!["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT"],
        "Legacy4" => vec!["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT"],
        _ => vec!["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"],
    }
}

// ─── Variants ────────────────────────────────────────────────────────────────

struct Variant;
impl Variant {
    const MR_CUM5: u8 = 0;
    const MR_CUM10: u8 = 1;
    const MR_CUM21: u8 = 2;
    const MR_DIFF5: u8 = 3;
    const MOM_CUM5: u8 = 4;
    const MOM_CUM10: u8 = 5;
    const MOM_CUM21: u8 = 6;
    const MOM_DIFF5: u8 = 7;
    const COUNT: u8 = 8;

    fn get_ofi(sd: &SymData, bar: usize, id: u8) -> f64 {
        match id {
            Self::MR_CUM5 | Self::MOM_CUM5 => sd.ofi_cum5.get(bar).copied().unwrap_or(0.0),
            Self::MR_CUM10 | Self::MOM_CUM10 => sd.ofi_cum10.get(bar).copied().unwrap_or(0.0),
            Self::MR_CUM21 | Self::MOM_CUM21 => sd.ofi_cum21.get(bar).copied().unwrap_or(0.0),
            Self::MR_DIFF5 | Self::MOM_DIFF5 => sd.ofi_diff5.get(bar).copied().unwrap_or(0.0),
            _ => 0.0,
        }
    }

    fn is_momentum(id: u8) -> bool {
        id >= Self::MOM_CUM5
    }
    fn label(id: u8) -> &'static str {
        match id {
            Self::MR_CUM5 => "MR_Cum5",
            Self::MR_CUM10 => "MR_Cum10",
            Self::MR_CUM21 => "MR_Cum21",
            Self::MR_DIFF5 => "MR_Diff5",
            Self::MOM_CUM5 => "Mom_Cum5",
            Self::MOM_CUM10 => "Mom_Cum10",
            Self::MOM_CUM21 => "Mom_Cum21",
            Self::MOM_DIFF5 => "Mom_Diff5",
            _ => "???",
        }
    }
}

// ─── Strategy ────────────────────────────────────────────────────────────────

fn run_strategy(
    data: &HashMap<String, SymData>,
    symbols: &[&str],
    start: usize,
    end: usize,
    variant_id: u8,
) -> Stats {
    let mut bar_rets: Vec<f64> = Vec::new();
    let n_active = N_POSITIONS as f64;
    let is_mom = Variant::is_momentum(variant_id);

    let mut bar = start;
    while bar + HOLD_BARS < end {
        // Build ranked list by OFI value
        let mut ranked: Vec<(&str, f64)> = Vec::new();
        for &sym in symbols {
            if let Some(sd) = data.get(sym) {
                if bar < sd.close.len() {
                    let val = Variant::get_ofi(sd, bar, variant_id);
                    if val.is_finite() {
                        ranked.push((sym, val));
                    }
                }
            }
        }

        if ranked.len() >= N_POSITIONS {
            ranked.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

            // Mean-reversion: long lowest, short highest
            // Momentum: long highest, short lowest
            let n_long = 2;
            let n_short = 2;

            let mut long_syms: Vec<&str> = if is_mom {
                ranked.iter().rev().take(n_long).map(|(s, _)| *s).collect()
            } else {
                ranked.iter().take(n_long).map(|(s, _)| *s).collect()
            };

            let short_syms: Vec<&str> = if is_mom {
                ranked.iter().take(n_short).map(|(s, _)| *s).collect()
            } else {
                ranked.iter().rev().take(n_short).map(|(s, _)| *s).collect()
            };

            let mut rets: Vec<f64> = Vec::new();

            for sym in &long_syms {
                if let Some(sd) = data.get(*sym) {
                    if bar + 1 < sd.close.len() && bar + HOLD_BARS < sd.close.len() {
                        let entry = sd.close[bar + 1];
                        let exit = sd.close[bar + HOLD_BARS];
                        rets.push((exit / entry - 1.0) - 2.0 * TAKER_FEE);
                    }
                }
            }

            for sym in &short_syms {
                if let Some(sd) = data.get(*sym) {
                    if bar + 1 < sd.close.len() && bar + HOLD_BARS < sd.close.len() {
                        let entry = sd.close[bar + 1];
                        let exit = sd.close[bar + HOLD_BARS];
                        rets.push(-(exit / entry - 1.0) - 2.0 * TAKER_FEE);
                    }
                }
            }

            if !rets.is_empty() {
                // Portfolio return = average across concurrent positions
                let avg: f64 = rets.iter().sum::<f64>() / n_active;
                bar_rets.push(avg);
            }
        }
        bar += 1;
    }

    compute_stats(&bar_rets)
}

#[derive(Clone)]
struct Stats {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    n_trades: usize,
    win_rate: f64,
    pass: bool,
}

fn compute_stats(bar_rets: &[f64]) -> Stats {
    if bar_rets.is_empty() {
        return Stats {
            ret: 0.0,
            sharpe: 0.0,
            max_dd: 0.0,
            n_trades: 0,
            win_rate: 0.0,
            pass: false,
        };
    }
    let n = bar_rets.len() as f64;
    let total: f64 = bar_rets.iter().sum();
    let wins: f64 = bar_rets.iter().filter(|&&t| t > 0.0).count() as f64;
    let mean = total / n;
    let var: f64 = bar_rets.iter().map(|&t| (t - mean).powi(2)).sum::<f64>() / n;
    let std = var.sqrt();
    let sharpe = if std > 1e-10 {
        mean / std * (252.0f64.sqrt())
    } else {
        0.0
    };

    let mut equity: f64 = 1.0;
    let mut peak: f64 = 1.0;
    let mut max_dd: f64 = 0.0;
    for &t in bar_rets {
        equity *= 1.0 + t;
        peak = peak.max(equity);
        let dd = (peak - equity) / peak;
        max_dd = max_dd.max(dd);
    }

    Stats {
        ret: total * 100.0,
        sharpe,
        max_dd: max_dd * 100.0,
        n_trades: bar_rets.len(),
        win_rate: wins / n * 100.0,
        pass: total > 0.0 && bar_rets.len() >= MIN_TRADES,
    }
}

// ─── CPCV ────────────────────────────────────────────────────────────────────

fn cpcv_positive_blocks(
    data: &HashMap<String, SymData>,
    symbols: &[&str],
    full_start: usize,
    full_end: usize,
    variant_id: u8,
) -> usize {
    let n_blocks = 15;
    let block_size = (full_end - full_start) / n_blocks;
    let mut total_pos = 0;

    for rep in 0..15 {
        let mut rng = (rep as u64).wrapping_mul(1103515245).wrapping_add(12345);
        let mut blocks: Vec<usize> = (0..n_blocks).collect();
        for i in (1..blocks.len()).rev() {
            rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
            let j = (rng % (i as u64 + 1)) as usize;
            blocks.swap(i, j);
        }
        let sample: Vec<usize> = blocks.into_iter().take(8).collect();

        let mut block_rets: Vec<f64> = Vec::new();
        for &b in &sample {
            let bs = full_start + b * block_size;
            let be = (bs + block_size).min(full_end);
            if be <= bs + HOLD_BARS {
                continue;
            }
            let s = run_strategy(data, symbols, bs, be, variant_id);
            if s.n_trades >= MIN_TRADES {
                block_rets.push(s.ret / 100.0);
            }
        }
        if block_rets.iter().sum::<f64>() > 0.0 {
            total_pos += 1;
        }
    }
    total_pos
}

// ─── Quarter windows ─────────────────────────────────────────────────────────

fn quarter_windows(total: usize) -> Vec<(usize, usize)> {
    let w = total / 4;
    vec![(0, w), (w, 2 * w), (2 * w, 3 * w), (3 * w, total)]
}

// ─── Main ────────────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let loader = DataLoader::new(None, None);
    let cache_dir = Path::new("data/cache");
    let universes = vec![
        "Base5",
        "NoDOGE",
        "OldGuardNoBNB",
        "LargeCaps5",
        "Legacy5BNB",
        "Legacy4",
    ];
    let variant_ids: Vec<u8> = (0..Variant::COUNT).collect();

    println!("\n=== OFI Microstructure Benchmark — Kira 2026-04-01 ===");
    println!("Signal: OFI mean-reversion + momentum | Exec: next-open, 21-bar hold, 0.1% taker");
    println!("Book: 2 long + 2 short | Portfolio avg return per bar\n");

    // Variant labels header
    let labels: Vec<String> = variant_ids
        .iter()
        .map(|&id| format!("{:>10}", Variant::label(id)))
        .collect();
    println!("  {:<12} {}", "", labels.join(" "));
    println!("  {}", "-".repeat(12 + 1 + variant_ids.len() * 11));

    for univ_name in &universes {
        let symbols = universe_symbols(univ_name);
        let mut data: HashMap<String, SymData> = HashMap::new();
        let mut min_len = usize::MAX;

        for &sym in &symbols {
            let sym_lower = sym.to_lowercase();
            let cache_path = cache_dir.join(format!("{}_1d.parquet", sym_lower));
            let df = if cache_path.exists() {
                DataLoader::load_parquet(&cache_path).ok()
            } else {
                loader.load_from_cache(sym, "1d").ok().flatten()
            };

            if let Some(df) = df {
                if let Ok(sd) = SymData::from_df(&df) {
                    min_len = min_len.min(sd.close.len());
                    data.insert(sym.to_string(), sd);
                }
            }
        }

        if data.len() != symbols.len() {
            println!(
                "\n  {}: insufficient data ({}/{} symbols)",
                univ_name,
                data.len(),
                symbols.len()
            );
            continue;
        }

        // Truncate
        for sd in data.values_mut() {
            sd.close.truncate(min_len);
            sd.ofi_cum5.truncate(min_len);
            sd.ofi_cum10.truncate(min_len);
            sd.ofi_cum21.truncate(min_len);
            sd.ofi_diff5.truncate(min_len);
        }

        let total = min_len;
        let warmup = 22;
        let full_start = warmup;
        let full_end = total.saturating_sub(HOLD_BARS);

        if full_end <= full_start + 252 {
            println!("\n  {}: skipping ({} bars)", univ_name, total);
            continue;
        }

        // Full-sample results
        let full_stats: Vec<Stats> = variant_ids
            .iter()
            .map(|&id| run_strategy(&data, &symbols, full_start, full_end, id))
            .collect();

        print!("  {:<12} ", univ_name);
        for s in &full_stats {
            print!(" {:>+9.0}%", s.ret);
        }
        println!();

        // Quarter windows (best variant only to save space)
        let qw = quarter_windows(full_end - full_start);
        for (qi, (ws, we)) in qw.iter().enumerate() {
            let qs = full_start + ws;
            let qe = (full_start + we).min(full_end);
            if qe <= qs + HOLD_BARS {
                continue;
            }

            print!("  {:<12} ", format!("Q{}", qi));
            for &id in &variant_ids {
                let s = run_strategy(&data, &symbols, qs, qe, id);
                print!(" {:>+9.0}%", s.ret);
            }
            println!();
        }

        // Pass counts per quarter
        print!("  {:<12} ", "Q_pass/4");
        for &id in &variant_ids {
            let mut passes = 0;
            for (ws, we) in &qw {
                let qs = full_start + ws;
                let qe = (full_start + we).min(full_end);
                if qe <= qs + HOLD_BARS {
                    continue;
                }
                let s = run_strategy(&data, &symbols, qs, qe, id);
                if s.pass {
                    passes += 1;
                }
            }
            print!(" {:>10}/4", passes);
        }
        println!();

        // CPCV
        print!("  {:<12} ", "CPCV/15");
        for &id in &variant_ids {
            let pos = cpcv_positive_blocks(&data, &symbols, full_start, full_end, id);
            print!(" {:>10}/15", pos);
        }
        println!();

        // Sharpe row
        print!("  {:<12} ", "Sharpe");
        for s in &full_stats {
            print!(" {:>10.2}", s.sharpe);
        }
        println!();

        // MaxDD row
        print!("  {:<12} ", "MaxDD%");
        for s in &full_stats {
            print!(" {:>10.1}", s.max_dd);
        }
        println!();

        // Trades row
        print!("  {:<12} ", "N_bars");
        for s in &full_stats {
            print!(" {:>10}", s.n_trades);
        }
        println!();

        println!();
    }

    println!("\n=== SUMMARY ===");
    println!("MR = mean-reversion: long LOWEST OFI (oversold), short HIGHEST OFI (overbought)");
    println!("Mom = momentum: long HIGHEST OFI (strength), short LOWEST OFI (weakness)");
    println!("");
    println!(
        "Result: both families expected to fail if OFI microstructure has no edge at daily bars."
    );
    println!("If Mom beats MR consistently → order flow momentum is real.");
    println!("If MR beats Mom consistently → order flow mean-reversion is real.");
    println!("Track C breadth goal: test genuinely different families.");

    Ok(())
}
