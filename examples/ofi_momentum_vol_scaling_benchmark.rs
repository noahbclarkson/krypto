//! OFI Momentum + Vol-Regime Position Sizing — Kira Research 2026-04-01
//!
//! Follow-up to ofi_microstructure_benchmark.rs
//!
//! Problem: OFI Momentum shows real Sharpe (1.51 on NoDOGE, 1.06 on Base5) but
//! MaxDD = 100% everywhere — no vol-scaling means catastrophic drawdowns.
//!
//! Architecture: bar-range approach (same as working original).
//! - At each bar, rank symbols by OFI momentum
//! - Enter long top-N and short bottom-N
//! - Hold for exactly HOLD_BARS, measure return bar+1 → bar+HOLD_BARS
//! - This avoids wash-trade problems of same-bar close-close entries
//!
//! Vol-regime sizing overlay:
//!   - 21-bar realized vol %-rank vs prior 252-bar history
//!   - low (< 0.25): 1.5× | mid-low (0.25-0.50): 1.25×
//!   - mid-high (0.50-0.75): 0.75× | high (>= 0.75): 0.5×

use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::path::Path;

const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 10;
const N_POSITIONS: usize = 2; // 2 long + 2 short (same as original)
const VOL_WINDOW: usize = 21;
const VOL_HIST: usize = 252;

// ─── Data ─────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    ofi_cum10: Vec<f64>,
    vol_pct_rank: Vec<f64>,
}

impl SymData {
    fn from_df(df: &DataFrame) -> Option<Self> {
        let n = df.height();
        let close = Self::col_f64(df, "close", n)?;
        let high = Self::col_f64(df, "high", n)?;
        let low = Self::col_f64(df, "low", n)?;

        // OFI = (close - midpoint) / range
        let mut ofi = vec![0.0; n];
        for i in 0..n {
            let range = high[i] - low[i];
            if range > 1e-9 {
                ofi[i] = (close[i] - (high[i] + low[i]) / 2.0) / range;
            }
        }

        let ofi_cum10 = Self::rolling_sum(&ofi, 10);
        let vol_pct_rank = Self::compute_vol_pct_rank(&close, n);

        Some(Self {
            close,
            ofi_cum10,
            vol_pct_rank,
        })
    }

    fn col_f64(df: &DataFrame, name: &str, n: usize) -> Option<Vec<f64>> {
        use polars::chunked_array::ChunkedArray;
        use polars::datatypes::Float64Type;
        let ca = df.column(name).ok()?.f64().ok()?;
        Some(ca.into_iter().map(|v| v.unwrap_or(0.0)).take(n).collect())
    }

    fn rolling_sum(v: &[f64], w: usize) -> Vec<f64> {
        let n = v.len();
        let mut out = vec![0.0; n];
        for i in w..n {
            out[i] = v[(i - w)..i].iter().sum();
        }
        out
    }

    fn compute_vol_pct_rank(close: &[f64], n: usize) -> Vec<f64> {
        let mut returns = vec![0.0; n];
        for i in 1..n {
            if close[i - 1] > 0.0 {
                returns[i] = (close[i] - close[i - 1]) / close[i - 1];
            }
        }

        let mut vol_pct_rank = vec![0.0; n];
        for i in VOL_HIST..n {
            // 21-bar vol at i
            let mut cur_sq = 0.0;
            for j in (i.saturating_sub(VOL_WINDOW))..i {
                cur_sq += returns[j] * returns[j];
            }
            let cur_vol = (cur_sq / VOL_WINDOW as f64).sqrt();

            // Count prior windows below current
            let ws = i.saturating_sub(VOL_HIST);
            let we = i.saturating_sub(VOL_WINDOW);
            let total = we.saturating_sub(ws).max(1);
            let mut below = 0usize;
            for w in ws..we {
                let mut hist_sq = 0.0;
                for j in w..(w + VOL_WINDOW) {
                    hist_sq += returns[j] * returns[j];
                }
                let hist_vol = (hist_sq / VOL_WINDOW as f64).sqrt();
                if hist_vol < cur_vol {
                    below += 1;
                }
            }
            vol_pct_rank[i] = below as f64 / total as f64;
        }
        vol_pct_rank
    }

    fn vol_mult(&self, bar: usize) -> f64 {
        if bar >= self.vol_pct_rank.len() {
            return 1.0;
        }
        let p = self.vol_pct_rank[bar];
        if p < 0.25 {
            1.5
        } else if p < 0.50 {
            1.25
        } else if p < 0.75 {
            0.75
        } else {
            0.5
        }
    }
}

// ─── Stats ────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct Stats {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    n_trades: usize,
    win_rate: f64,
    equity_curve: Vec<f64>,
}

fn compute_stats(bar_rets: &[f64], vol_mults: &[f64]) -> Stats {
    if bar_rets.is_empty() {
        return Stats {
            ret: -1.0,
            sharpe: 0.0,
            max_dd: 1.0,
            n_trades: 0,
            win_rate: 0.0,
            equity_curve: vec![],
        };
    }

    let n = bar_rets.len();
    let mut equity = 1.0f64;
    let mut peak = 1.0f64;
    let mut max_dd = 0.0f64;
    let mut wins = 0f64;
    let mut equity_curve = vec![1.0; n];

    for i in 0..n {
        let vm = if i < vol_mults.len() {
            vol_mults[i]
        } else {
            1.0
        };
        let ret = bar_rets[i] * vm; // vol-scaled return
        equity *= 1.0 + ret;
        equity = equity.max(0.0);
        equity_curve[i] = equity;
        peak = peak.max(equity);
        let dd = (peak - equity) / peak;
        if dd > max_dd {
            max_dd = dd;
        }
        if ret > 0.0 {
            wins += 1.0;
        }
    }

    let ret = equity - 1.0;
    let mean_ret = bar_rets.iter().sum::<f64>() / n as f64;
    let std_ret = (bar_rets
        .iter()
        .map(|r| {
            let d = r - mean_ret;
            d * d
        })
        .sum::<f64>()
        / n as f64)
        .sqrt();
    let sharpe = if std_ret > 1e-9 {
        mean_ret / std_ret * (n as f64).sqrt()
    } else {
        0.0
    };
    let win_rate = wins / n as f64;
    let n_trades = n;

    Stats {
        ret,
        sharpe,
        max_dd,
        n_trades,
        win_rate,
        equity_curve,
    }
}

fn compute_stats_base(bar_rets: &[f64]) -> Stats {
    if bar_rets.is_empty() {
        return Stats {
            ret: -1.0,
            sharpe: 0.0,
            max_dd: 1.0,
            n_trades: 0,
            win_rate: 0.0,
            equity_curve: vec![],
        };
    }
    let n = bar_rets.len();
    let mut equity = 1.0f64;
    let mut peak = 1.0f64;
    let mut max_dd = 0.0f64;
    let mut wins = 0f64;
    for r in bar_rets {
        equity *= 1.0 + r;
        equity = equity.max(0.0);
        peak = peak.max(equity);
        let dd = (peak - equity) / peak;
        if dd > max_dd {
            max_dd = dd;
        }
        if *r > 0.0 {
            wins += 1.0;
        }
    }
    let ret = equity - 1.0;
    let mean_ret = bar_rets.iter().sum::<f64>() / n as f64;
    let std_ret = (bar_rets
        .iter()
        .map(|r| {
            let d = r - mean_ret;
            d * d
        })
        .sum::<f64>()
        / n as f64)
        .sqrt();
    let sharpe = if std_ret > 1e-9 {
        mean_ret / std_ret * (n as f64).sqrt()
    } else {
        0.0
    };
    let win_rate = wins / n as f64;
    Stats {
        ret,
        sharpe,
        max_dd,
        n_trades: n,
        win_rate,
        equity_curve: vec![],
    }
}

// ─── Strategy (bar-range approach, same as working original) ─────────────────

fn run_strategy(
    data: &HashMap<String, SymData>,
    symbols: &[&str],
    start: usize,
    end: usize,
    use_vol_scale: bool,
) -> (Vec<f64>, Vec<f64>) {
    // bar_rets = portfolio return each rebalance bar
    // vol_mults = vol multiplier each bar (for vol-scaled version)
    let mut bar_rets = Vec::new();
    let mut vol_mults = Vec::new();
    let n_active = N_POSITIONS as f64;

    let mut bar = start;
    while bar + HOLD_BARS < end {
        // Rank by OFI momentum
        let mut ranked: Vec<(&str, f64)> = Vec::new();
        for &sym in symbols {
            if let Some(sd) = data.get(sym) {
                if bar < sd.ofi_cum10.len() && sd.ofi_cum10[bar].is_finite() {
                    ranked.push((sym, sd.ofi_cum10[bar]));
                }
            }
        }

        if ranked.len() >= N_POSITIONS * 2 {
            ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

            // Momentum: long highest OFI, short lowest
            let n_long = N_POSITIONS;
            let n_short = N_POSITIONS;

            let long_syms: Vec<String> = ranked
                .iter()
                .rev()
                .take(n_long)
                .map(|(s, _)| (*s).to_string())
                .collect();
            let short_syms: Vec<String> = ranked
                .iter()
                .take(n_short)
                .map(|(s, _)| (*s).to_string())
                .collect();

            // Compute avg vol multiplier for this bar (for vol-scaled version)
            let avg_vol_mult = if use_vol_scale {
                let mut sum = 0.0f64;
                let mut cnt = 0usize;
                for sym in long_syms.iter().chain(short_syms.iter()) {
                    if let Some(sd) = data.get(sym) {
                        sum += sd.vol_mult(bar);
                        cnt += 1;
                    }
                }
                if cnt > 0 {
                    sum / cnt as f64
                } else {
                    1.0
                }
            } else {
                1.0
            };

            // Measure return from bar+1 to bar+HOLD_BARS
            let mut rets: Vec<f64> = Vec::new();
            for sym in &long_syms {
                if let Some(sd) = data.get(sym) {
                    if bar + 1 < sd.close.len() && bar + HOLD_BARS < sd.close.len() {
                        let entry = sd.close[bar + 1];
                        let exit = sd.close[bar + HOLD_BARS];
                        let gross = exit / entry - 1.0;
                        let net = gross - 2.0 * TAKER_FEE;
                        rets.push(net);
                    }
                }
            }
            for sym in &short_syms {
                if let Some(sd) = data.get(sym) {
                    if bar + 1 < sd.close.len() && bar + HOLD_BARS < sd.close.len() {
                        let entry = sd.close[bar + 1];
                        let exit = sd.close[bar + HOLD_BARS];
                        let gross = exit / entry - 1.0;
                        let net = -(gross + 2.0 * TAKER_FEE);
                        rets.push(net);
                    }
                }
            }

            if !rets.is_empty() {
                let avg: f64 = rets.iter().sum::<f64>() / n_active;
                bar_rets.push(avg);
                vol_mults.push(avg_vol_mult);
            } else {
                bar_rets.push(0.0);
                vol_mults.push(1.0);
            }
        } else {
            bar_rets.push(0.0);
            vol_mults.push(1.0);
        }

        bar += 1;
    }

    (bar_rets, vol_mults)
}

// ─── Quarterly / CPCV ────────────────────────────────────────────────────────

fn run_universe(
    data_dir: &Path,
    universe: &str,
    use_vol_scale: bool,
) -> (
    String,
    f64,
    usize,
    f64,
    f64,
    Vec<(bool, f64, usize)>,
    Vec<(bool, f64)>,
) {
    // Load data — use same approach as original working OFI benchmark
    let univ_symbols: Vec<&str> = match universe {
        "Base5" => vec!["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"],
        "NoDOGE" => vec!["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "BNBUSDT"],
        "OldGuardNoBNB" => vec!["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BCHUSDT"],
        "LargeCaps5" => vec!["BTCUSDT", "ETHUSDT", "BNBUSDT", "XRPUSDT", "LTCUSDT"],
        "Legacy5BNB" => vec!["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT"],
        "Legacy4" => vec!["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT"],
        _ => vec!["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"],
    };

    let mut data: HashMap<String, SymData> = HashMap::new();
    let mut min_len = usize::MAX;

    for sym in &univ_symbols {
        let sym_lower = sym.to_lowercase();
        let path = data_dir.join(format!("{}_1d.parquet", sym_lower));
        let df = DataLoader::load_parquet(&path).ok();
        if let Some(df) = df {
            if let Some(sd) = SymData::from_df(&df) {
                min_len = min_len.min(sd.close.len());
                data.insert(sym.to_string(), sd);
            }
        }
    }

    // Skip universe if any symbol is missing (survivorship would corrupt rankings)
    if data.len() < univ_symbols.len() {
        eprintln!(
            "[WARN] {} skipped: only {}/{} symbols loaded",
            universe,
            data.len(),
            univ_symbols.len()
        );
        return (universe.to_string(), 0.0, 0, 0.0, 0.0, vec![], vec![]);
    }

    // Truncate all to min_len
    for sd in data.values_mut() {
        sd.close.truncate(min_len);
        sd.ofi_cum10.truncate(min_len);
        sd.vol_pct_rank.truncate(min_len);
    }

    let end = min_len;

    // Full period
    let (full_rets, full_vm) = run_strategy(&data, &univ_symbols, VOL_HIST, end, use_vol_scale);
    let stats = if use_vol_scale {
        compute_stats(&full_rets, &full_vm)
    } else {
        compute_stats_base(&full_rets)
    };

    // Quarterly
    let q_len = end.saturating_sub(VOL_HIST) / 4;
    let mut q_results = Vec::new();
    for q in 0..4 {
        let q_start = VOL_HIST + q * q_len;
        let q_end = if q < 3 {
            VOL_HIST + (q + 1) * q_len
        } else {
            end
        };
        if q_end <= q_start {
            continue;
        }
        let (q_rets, q_vm) = run_strategy(&data, &univ_symbols, q_start, q_end, use_vol_scale);
        if q_rets.len() < MIN_TRADES {
            continue;
        }
        let qs = if use_vol_scale {
            compute_stats(&q_rets, &q_vm)
        } else {
            compute_stats_base(&q_rets)
        };
        q_results.push((qs.ret > 0.0, qs.ret, qs.n_trades));
    }

    // CPCV block bootstrap
    let mut cpcv_results = Vec::new();
    let active_len = end.saturating_sub(VOL_HIST);
    if active_len > 500 {
        let block_size = active_len / 6;
        for _ in 0..15 {
            let b0 = ((rand_u64() % 6) as usize);
            let b1 = ((rand_u64() % 6) as usize);
            let b2 = ((rand_u64() % 6) as usize);
            let b3 = ((rand_u64() % 6) as usize);
            let mut equity = 1.0f64;
            let mut total_trades = 0usize;
            for &b in &[b0, b1, b2, b3] {
                let s = VOL_HIST + b * block_size;
                let e = (VOL_HIST + (b + 1) * block_size).min(end);
                if e <= s {
                    continue;
                }
                let (br, vm) = run_strategy(&data, &univ_symbols, s, e, use_vol_scale);
                total_trades += br.len();
                let s = if use_vol_scale {
                    compute_stats(&br, &vm)
                } else {
                    compute_stats_base(&br)
                };
                equity *= 1.0 + s.ret;
            }
            let passed = equity > 1.0 && total_trades >= MIN_TRADES;
            cpcv_results.push((passed, (equity - 1.0) * 100.0));
        }
    }

    (
        universe.to_string(),
        stats.ret * 100.0,
        stats.n_trades,
        stats.sharpe,
        stats.max_dd * 100.0,
        q_results,
        cpcv_results,
    )
}

fn rand_u64() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEED: AtomicU64 = AtomicU64::new(42);
    let prev = SEED.load(Ordering::Relaxed);
    let next = prev.wrapping_mul(6364136223846793005).wrapping_add(1);
    SEED.store(next, Ordering::Relaxed);
    next
}

// ─── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    let data_dir = Path::new("/home/ubuntu/.openclaw/workspace-krypto/krypto/data/cache");
    let universes = vec![
        "Base5",
        "NoDOGE",
        "OldGuardNoBNB",
        "LargeCaps5",
        "Legacy5BNB",
        "Legacy4",
    ];

    println!("\n=== OFI Momentum + Vol-Regime Sizing ===\n");
    println!(
        "{:<18} {:>9} {:>7} {:>7} {:>7} {:>4} {:>4} {:>9}",
        "Universe", "Return%", "Trades", "Sharpe", "MaxDD%", "QP", "CPCV", "Strategy"
    );
    println!("{}", "-".repeat(75));

    let mut all_base = Vec::new();
    let mut all_vol = Vec::new();

    for universe in &universes {
        // BASE
        let (u, ret_b, trades_b, sharpe_b, dd_b, q_b, r_b) =
            run_universe(data_dir, universe, false);
        let qp_b = q_b.iter().filter(|(p, _, _)| *p).count();
        let cp_b = r_b.iter().filter(|(p, _)| *p).count();

        // VOL
        let (u2, ret_v, trades_v, sharpe_v, dd_v, q_v, r_v) =
            run_universe(data_dir, universe, true);
        let qp_v = q_v.iter().filter(|(p, _, _)| *p).count();
        let cp_v = r_v.iter().filter(|(p, _)| *p).count();

        let b_score = qp_b * 4 + cp_b;
        let v_score = qp_v * 4 + cp_v;
        let winner = if b_score >= v_score { "BASE" } else { "VOL" };

        println!(
            "{:<18} {:>9.1} {:>7} {:>7.2} {:>7.1} {:>4} {:>4} {:>9}",
            u, ret_b, trades_b, sharpe_b, dd_b, qp_b, cp_b, "BASE"
        );
        let qb_str: String = q_b
            .iter()
            .map(|(p, r, _)| format!("{}{:.0}%", if *p { "Y" } else { "N" }, r))
            .collect();
        println!("  BASE Q=[{}] CPCV={}/{}", qb_str, cp_b, 15);

        println!(
            "{:<18} {:>9.1} {:>7} {:>7.2} {:>7.1} {:>4} {:>4} {:>9}",
            u2, ret_v, trades_v, sharpe_v, dd_v, qp_v, cp_v, "VOL"
        );
        let qv_str: String = q_v
            .iter()
            .map(|(p, r, _)| format!("{}{:.0}%", if *p { "Y" } else { "N" }, r))
            .collect();
        println!("  VOL  Q=[{}] CPCV={}/{}", qv_str, cp_v, 15);

        println!(
            "  => {} | DD: {:.1}% -> {:.1}% | Sharpe: {:.2} -> {:.2}",
            winner, dd_b, dd_v, sharpe_b, sharpe_v
        );

        all_base.push((u.clone(), ret_b, trades_b, sharpe_b, dd_b, qp_b, cp_b));
        all_vol.push((u2.clone(), ret_v, trades_v, sharpe_v, dd_v, qp_v, cp_v));
    }

    println!("\n{}", "=".repeat(75));
    println!("SUMMARY\n");
    println!(
        "{:<18} {:>9} {:>9} {:>7} {:>7} {:>7} {:>7}",
        "Universe", "BaseRet%", "VolRet%", "BaseDD%", "VolDD%", "BaseSh", "VolSh"
    );
    println!("{}", "-".repeat(75));
    for (i, (u, ret_b, _, sharpe_b, dd_b, _, _)) in all_base.iter().enumerate() {
        let (_, ret_v, _, sharpe_v, dd_v, _, _) = &all_vol[i];
        println!(
            "{:<18} {:>9.1} {:>9.1} {:>7.1} {:>7.1} {:>7.2} {:>7.2}",
            u, ret_b, ret_v, dd_b, dd_v, sharpe_b, sharpe_v
        );
    }

    let vol_wins = all_vol
        .iter()
        .zip(all_base.iter())
        .filter(|(v, b)| (v.5 * 4 + v.6) > (b.5 * 4 + b.6))
        .count();
    let dd_improves = all_vol
        .iter()
        .zip(all_base.iter())
        .filter(|(v, b)| v.4 < b.4)
        .count();
    let sharpe_improves = all_vol
        .iter()
        .zip(all_base.iter())
        .filter(|(v, b)| v.3 > b.3)
        .count();

    println!(
        "\nVol-scaled wins chronology: {}/{}",
        vol_wins,
        universes.len()
    );
    println!(
        "Vol-scaled improves MaxDD: {}/{}",
        dd_improves,
        universes.len()
    );
    println!(
        "Vol-scaled improves Sharpe: {}/{}",
        sharpe_improves,
        universes.len()
    );
}
