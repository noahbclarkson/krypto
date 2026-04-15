//! Perp-Spot Basis Carry V2 — Kira Research 2026-04-01
//!
//! BIS carry: annualized funding rate rank across assets.
//!   Long LOW funding (cheap carry), Short HIGH funding (expensive carry)
//!   Carry PnL = annual_fund_rate * hold_periods / 1095
//!   Price PnL = mark price change over hold

use anyhow::Result;
use krypto::data::funding_rate::FundingRateLoader;
use polars::prelude::*;
use std::time::Instant;

const PERP_SYMBOLS: [&str; 3] = ["BTCUSDT", "ETHUSDT", "SOLUSDT"];
const FEE_PER_SIDE: f64 = 0.001;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const MIN_TRADES: usize = 3;

fn rolling_z(values: &[f64], window: usize) -> Vec<f64> {
    let n = values.len();
    let mut z = vec![0.0; n];
    for i in window..n {
        let slice = &values[i - window..i];
        let mean = slice.iter().sum::<f64>() / window as f64;
        let var = slice.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (window - 1) as f64;
        let std = var.sqrt();
        z[i] = if std > 1e-10 {
            (values[i] - mean) / std
        } else {
            0.0
        };
    }
    z
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("═══ Perp-Spot Basis Carry V2 ═══");
    println!("Strategy: annualized funding rate rank carry\n");

    let funding_cache_dir = "examples/funding_cache";

    // Load funding data
    let t0 = Instant::now();
    let mut all_ann: Vec<Vec<f64>> = Vec::new();
    let mut all_mark: Vec<Vec<f64>> = Vec::new();

    for sym in PERP_SYMBOLS {
        let loader = FundingRateLoader::with_cache_dir(funding_cache_dir);
        let df: DataFrame = loader.fetch(sym, None, None).await?;
        let rates: Vec<f64> = df
            .column("funding_rate")?
            .f64()?
            .into_iter()
            .flatten()
            .collect();
        let marks: Vec<f64> = df
            .column("mark_price")?
            .f64()?
            .into_iter()
            .flatten()
            .collect();
        let ann: Vec<f64> = rates.iter().map(|r| r * 1095.0).collect();
        let r_min = rates.iter().fold(f64::INFINITY, |a, &b| a.min(b));
        let r_max = rates.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
        println!(
            "  {sym}: {} records | rate [{:.4}, {:.4}]",
            rates.len(),
            r_min,
            r_max
        );
        all_ann.push(ann);
        all_mark.push(marks);
    }

    let min_len = all_ann.iter().map(|v| v.len()).min().unwrap_or(0);
    let n_total = min_len;
    for s in &mut all_ann {
        s.truncate(n_total);
    }
    for s in &mut all_mark {
        s.truncate(n_total);
    }
    println!(
        "\nCommon: {n_total} records (~{:.0} days) | {:.1}s\n",
        n_total as f64 / 3.0,
        t0.elapsed().as_secs_f32()
    );

    // Walk-Forward
    let z_windows: [usize; 3] = [30, 60, 90];
    let holds: [usize; 3] = [6, 10, 21];

    let mut best: Option<(usize, usize, usize, usize, f64, f64, f64)> = None;
    let mut best_sh: f64 = f64::NEG_INFINITY;

    for &z_w in &z_windows {
        for &hold in &holds {
            let t1 = Instant::now();
            let n_win = n_total.saturating_sub(TRAIN_BARS) / TEST_BARS;
            let mut n_pass = 0usize;
            let mut tot_ret: f64 = 0.0;
            let mut tot_sh: f64 = 0.0;
            let mut tot_dd: f64 = 0.0;
            let mut tot_trd: f64 = 0.0;
            let mut n_val = 0usize;

            for wi in 0..n_win {
                let t_start = TRAIN_BARS + wi * TEST_BARS;
                let t_end = (t_start + TEST_BARS).min(n_total);
                let t_len = t_end - t_start;
                if t_len < hold + 2 || t_start < z_w + 10 {
                    continue;
                }

                // Compute z-scores for test window
                let mut test_z: Vec<Vec<f64>> = Vec::new();
                for series in &all_ann {
                    let full_z = rolling_z(series, z_w);
                    let slice: Vec<f64> = (t_start..t_end)
                        .map(|i| *full_z.get(i).unwrap_or(&0.0))
                        .collect();
                    test_z.push(slice);
                }

                // Sim
                let mut equity: f64 = 1.0;
                let mut peak: f64 = 1.0;
                let mut max_dd: f64 = 0.0;
                let mut trades: usize = 0;
                let mut bar_rets: Vec<f64> = Vec::new();

                // (sym_idx, side(+1=long/-1=short), entry_fund_ann, entry_bar_idx)
                let mut positions: Vec<(usize, i8, f64, usize)> = Vec::new();

                for li in 0..t_len {
                    let bar_global = t_start + li;

                    // ── Exit matured positions ────────────────────────────────
                    // Split: matured vs still-open
                    let mut matured: Vec<(usize, i8, f64, usize)> = Vec::new();
                    let mut keep: Vec<(usize, i8, f64, usize)> = Vec::new();
                    for p in positions.drain(..) {
                        if li >= hold && (li - p.3) >= hold {
                            matured.push(p);
                        } else {
                            keep.push(p);
                        }
                    }
                    positions = keep;

                    if !matured.is_empty() {
                        let n2 = matured.len();
                        let mut sum_ret: f64 = 0.0;

                        for p in &matured {
                            let (sym, side, ef, eb) = p;
                            let sym_us: usize = *sym;
                            let side_i: i8 = *side;
                            let eb_us: usize = *eb;

                            // Carry: annual fund rate scaled to hold duration
                            let carry: f64 = if side_i > 0 {
                                ef * (hold as f64 / 1095.0)
                            } else {
                                -ef * (hold as f64 / 1095.0)
                            };

                            // Price return
                            let e_mark: f64 = *all_mark[sym_us].get(eb_us).unwrap_or(&1.0);
                            let x_mark: f64 = *all_mark[sym_us].get(bar_global).unwrap_or(&1.0);
                            let pret: f64 = if e_mark > 0.0 {
                                (x_mark - e_mark) / e_mark
                            } else {
                                0.0
                            };
                            let pctr: f64 = if side_i > 0 { pret } else { -pret };

                            sum_ret += carry + pctr;
                        }

                        let avg_ret: f64 = sum_ret / n2 as f64 - FEE_PER_SIDE * 2.0;
                        equity *= 1.0 + avg_ret;
                        trades += 1;
                        bar_rets.push(avg_ret);
                    }

                    // ── Enter new positions at this bar ───────────────────────
                    let entry_bar = li;
                    let mut ranked: Vec<(usize, f64)> = Vec::new();
                    for (si, z_series) in test_z.iter().enumerate() {
                        let zval: f64 = *z_series.get(entry_bar).unwrap_or(&0.0);
                        if zval.is_finite() {
                            ranked.push((si, zval));
                        }
                    }

                    if ranked.len() >= 2 {
                        ranked.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
                        // Lowest z = cheapest carry → LONG
                        // Highest z = most expensive carry → SHORT
                        let n_r = ranked.len();
                        let long_sym: usize = ranked[0].0;
                        let short_sym: usize = ranked[n_r - 1].0;

                        if long_sym != short_sym {
                            let lf: f64 = *all_ann[long_sym].get(entry_bar).unwrap_or(&0.0);
                            let hf: f64 = *all_ann[short_sym].get(entry_bar).unwrap_or(&0.0);
                            positions.push((long_sym, 1, lf, entry_bar));
                            positions.push((short_sym, -1, hf, entry_bar));
                        }
                    }

                    // ── Equity tracking ──────────────────────────────────────
                    peak = peak.max(equity);
                    let dd: f64 = (equity - peak) / peak;
                    if dd < max_dd {
                        max_dd = dd;
                    }
                }

                // Drop any open positions at end of window
                for p in positions.drain(..) {
                    let (sym, side, ef, eb) = p;
                    let sym_us: usize = sym;
                    let side_i: i8 = side;
                    let eb_us: usize = eb;
                    let carry: f64 = if side_i > 0 {
                        ef * (hold as f64 / 1095.0)
                    } else {
                        -ef * (hold as f64 / 1095.0)
                    };
                    let e_mark: f64 = *all_mark[sym_us].get(eb_us).unwrap_or(&1.0);
                    let x_mark: f64 = *all_mark[sym_us].last().unwrap_or(&1.0);
                    let pret: f64 = if e_mark > 0.0 {
                        (x_mark - e_mark) / e_mark
                    } else {
                        0.0
                    };
                    equity *=
                        1.0 + (carry + if side_i > 0 { pret } else { -pret }) - FEE_PER_SIDE * 2.0;
                    trades += 1;
                }

                if trades < MIN_TRADES {
                    continue;
                }
                n_val += 1;

                let ret_pct: f64 = (equity - 1.0) * 100.0;
                let sh: f64 = if bar_rets.len() > 1 {
                    let m: f64 = bar_rets.iter().sum::<f64>() / bar_rets.len() as f64;
                    let v: f64 = bar_rets.iter().map(|r| (r - m).powi(2)).sum::<f64>()
                        / (bar_rets.len() - 1) as f64;
                    let s: f64 = v.sqrt();
                    if s > 1e-10 {
                        m / s * 252f64.sqrt()
                    } else {
                        0.0
                    }
                } else {
                    0.0
                };

                if ret_pct > 0.0 {
                    n_pass += 1;
                }
                tot_ret += ret_pct;
                tot_sh += sh;
                tot_dd += max_dd * 100.0;
                tot_trd += trades as f64;
            }

            if n_val == 0 {
                continue;
            }
            let av_r = tot_ret / n_val as f64;
            let av_sh = tot_sh / n_val as f64;
            let av_dd = tot_dd / n_val as f64;
            let av_tr = tot_trd / n_val as f64;

            println!(
            "z={z_w:>3} h={hold:>2} | {n_pass:>2}/{n_val} pass | ret {:>+8.1}% | sh {:>+6.2} | dd {:>7.1}% | {:>4.0} trd | {:.2}s",
            av_r, av_sh, av_dd, av_tr, t1.elapsed().as_secs_f32()
        );

            if av_sh > best_sh {
                best_sh = av_sh;
                best = Some((z_w, hold, n_pass, n_val, av_r, av_sh, av_dd));
            }
        }
    }

    println!("\n═══ Summary ═══");
    if let Some((z_w, hold, passes, n_v, av_r, av_sh, av_dd)) = best {
        println!(
            "Best: z={z_w}, hold={hold} ({}h) | {passes}/{n_v} pass",
            hold * 8
        );
        println!(
            "  Return: {:+.1}% | Sharpe: {:+.2} | DD: {:.1}%",
            av_r, av_sh, av_dd
        );
    }
    println!("\nInterpretation: annualized funding rate rank as carry signal.");
    println!("If negative: funding acts as momentum signal, BIS carry thesis fails for crypto.");
    Ok(())
}
