//! Walk-Forward Stress Test: MACD+Regime vs Turtle+MACD on OldGuardNoBNB
//!
//! Same design as Base5 walk-forwards but on the harsher survivorship-basket:
//!   BTCUSDT, ETHUSDT, XRPUSDT, LTCUSDT, EOSUSDT, BCHUSDT
//!
//! This is the never-tested Track B question: do the OOS conclusions
//! from Base5 hold on the old-guard / legacy-heavy universe?
//!
//! Design: 252-bar train / 252-bar test / 21-bar hold / 0.1% taker each side

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const CANDLES: u32 = 3000;
const TURTLE_ENTRY: usize = 21; // hyperopt 2026-04-10: full 5-100 sweep found EP=21 is global max Sharpe (0.176) and most robust (78% universes positive)

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
}

impl SymData {
    fn from_df(df: &DataFrame) -> Result<Self> {
        let n = df.height();
        let close_ch = df.column("close")?.f64()?;
        let open_ch = df.column("open")?.f64()?;
        let high_ch = df.column("high")?.f64()?;
        let low_ch = df.column("low")?.f64()?;
        let macd_ch = df.column("macd")?.f64()?;
        let macd_sig_ch = df.column("macd_signal")?.f64()?;

        let close: Vec<f64> = (0..n).map(|i| close_ch.get(i).unwrap_or(0.0)).collect();
        let open: Vec<f64> = (0..n).map(|i| open_ch.get(i).unwrap_or(0.0)).collect();
        let high: Vec<f64> = (0..n).map(|i| high_ch.get(i).unwrap_or(0.0)).collect();
        let low: Vec<f64> = (0..n).map(|i| low_ch.get(i).unwrap_or(0.0)).collect();
        let macd: Vec<f64> = (0..n).map(|i| macd_ch.get(i).unwrap_or(0.0)).collect();
        let macd_signal: Vec<f64> = (0..n).map(|i| macd_sig_ch.get(i).unwrap_or(0.0)).collect();

        // Per-symbol SMA200
        let mut sma200 = vec![0.0; n];
        for i in 200..n {
            let sum: f64 = close[(i - 200)..i].iter().sum();
            sma200[i] = sum / 200.0;
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
        })
    }

    fn macd_gap(&self, i: usize) -> f64 {
        if i == 0 {
            return 0.0;
        }
        (self.macd[i] - self.macd_signal[i]).abs()
    }

    /// MACD+Regime signal at bar i (uses BTC SMA200 passed in)
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

    /// Turtle+MACD signal at bar i
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
}

fn run_backtest_with_sigs(
    data: &HashMap<String, SymData>,
    signals: &HashMap<String, Vec<i32>>,
    start: usize,
    end: usize,
) -> (f64, f64, f64, usize) {
    if end <= start || end - start < HOLD_BARS + 2 {
        return (0.0, 0.0, 0.0, 0);
    }

    let sym0 = data.keys().next().unwrap();
    let n = data.get(sym0).unwrap().close.len();
    let eff_end = end.min(n);

    let mut equity = 1.0_f64;
    let mut peak = equity;
    let mut max_dd = 0.0_f64;
    let mut trades = 0usize;
    let mut rets: Vec<f64> = Vec::new();
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
                if bar == 0 {
                    continue;
                }
                let sig = *sigs.get(bar.saturating_sub(1)).unwrap_or(&0);
                if sig == 0 {
                    continue;
                }
                cand.push((sym.clone(), sd.macd_gap(bar.saturating_sub(1))));
            }
            cand.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            for (sym, _) in cand.into_iter().take(3) {
                let sd = data.get(&sym).unwrap();
                let sigs = signals.get(&sym).unwrap();
                let sig = *sigs.get(bar.saturating_sub(1)).unwrap_or(&0);
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
                    let net = gross - 2.0 * TAKER_FEE;
                    rets.push(net);
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
        return (0.0, 0.0, 0.0, 0);
    }
    let ret_pct = (equity - 1.0) * 100.0;
    let avg = rets.iter().sum::<f64>() / trades as f64;
    let var = rets.iter().map(|r| (r - avg).powi(2)).sum::<f64>() / trades as f64;
    let sd = var.sqrt().max(1e-10);
    let sh = (avg / sd) * (252.0_f64).sqrt();
    (ret_pct, sh, -max_dd, trades)
}

#[derive(Clone)]
struct WfRec {
    wi: usize,
    period: &'static str,
    macd_trades: usize,
    macd_return: f64,
    macd_sharpe: f64,
    macd_dd: f64,
    macd_passed: bool,
    turtle_trades: usize,
    turtle_return: f64,
    turtle_sharpe: f64,
    turtle_dd: f64,
    turtle_passed: bool,
}

impl WfRec {
    fn winner(&self) -> &'static str {
        if self.turtle_return > self.macd_return {
            "Turtle+MACD"
        } else if self.macd_return > self.turtle_return {
            "MACD+Regime"
        } else {
            "TIE"
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("═══ Walk-Forward Stress: OldGuardNoBNB ═══");
    println!("MACD+Regime vs Turtle+MACD — same fair harness\n");

    let loader = DataLoader::new(None, None);
    // OldGuardNoBNB: survivorship-harsher basket
    let syms = [
        "BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT",
    ];

    let mut cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for s in &syms {
        let sk = s.to_string();
        let raw = loader.fetch_with_cache(&sk, "1d", CANDLES).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        min_len = min_len.min(df.height());
        cache.insert(sk, df);
    }

    // BTC data for regime SMA200 (market-wide regime for all symbols)
    let btc_raw = loader.fetch_with_cache("BTCUSDT", "1d", CANDLES).await?;
    let btc_df = FeatureEngine::add_technicals(&btc_raw, None)?;
    let btc_sd = SymData::from_df(&btc_df)?;

    let n = min_len.min(2800);
    for (_, df) in &mut cache {
        if df.height() > n {
            *df = df.slice(0, n);
        }
    }

    let total_windows = n.saturating_sub(TRAIN_BARS) / TEST_BARS;
    println!(
        "{} syms, {} bars (~{:.0} years), {} windows\n",
        syms.len(),
        n,
        n as f64 / 365.0,
        total_windows
    );

    let mut records: Vec<WfRec> = Vec::new();

    let period_labels = [
        "2017-18 Bull",
        "2018-19 Bear",
        "2019-20 Chop",
        "2020-21 MegaBull",
        "2021-22 MtGox/War",
        "2022-23 Bear",
        "2023-24 Recovery",
    ];

    for wi in 0..total_windows {
        let train_end = TRAIN_BARS + wi * TEST_BARS;
        let tstart = train_end;
        let tend = (tstart + TEST_BARS).min(n);
        if tend - tstart < HOLD_BARS + 2 {
            continue;
        }

        let t0 = Instant::now();

        // Build per-symbol data
        let mut sym_data: HashMap<String, SymData> = HashMap::new();
        for s in &syms {
            let sk = s.to_string();
            let df = cache.get(&sk).expect("symbol not in cache");
            sym_data.insert(sk.clone(), SymData::from_df(df)?);
        }

        // ── MACD+Regime signals (uses BTC SMA200) ──────────────
        let mut macd_signals: HashMap<String, Vec<i32>> = HashMap::new();
        for s in &syms {
            let sk = s.to_string();
            let sd = sym_data.get(&sk).unwrap();
            let mut sigs = vec![0i32; sd.close.len()];
            for i in 0..sd.close.len() {
                sigs[i] = sd.macd_regime_sig(i, btc_sd.sma200[i]);
            }
            macd_signals.insert(sk, sigs);
        }

        // ── Turtle+MACD signals ──────────────────────────────────
        let mut turtle_signals: HashMap<String, Vec<i32>> = HashMap::new();
        for s in &syms {
            let sk = s.to_string();
            let sd = sym_data.get(&sk).unwrap();
            let mut sigs = vec![0i32; sd.close.len()];
            for i in 0..sd.close.len() {
                sigs[i] = sd.turtle_macd_sig(i);
            }
            turtle_signals.insert(sk, sigs);
        }

        let (macd_ret, macd_sh, macd_dd, macd_trades) =
            run_backtest_with_sigs(&sym_data, &macd_signals, tstart, tend);
        let (turtle_ret, turtle_sh, turtle_dd, turtle_trades) =
            run_backtest_with_sigs(&sym_data, &turtle_signals, tstart, tend);

        let macd_passed = macd_trades >= MIN_TRADES && macd_ret > 0.0;
        let turtle_passed = turtle_trades >= MIN_TRADES && turtle_ret > 0.0;
        let period = period_labels.get(wi).copied().unwrap_or("Unknown");

        let winner_sym = if turtle_ret > macd_ret {
            "🐢 TM"
        } else if macd_ret > turtle_ret {
            "📊 MR"
        } else {
            "—  "
        };

        println!("  W{:02} {:14} | MR {:3}t {:+8.1}% sh={:5.2} DD={:5.1}% | TM {:3}t {:+8.1}% sh={:5.2} DD={:5.1}% | {} | {:.1}s",
            wi, period, macd_trades, macd_ret, macd_sh, macd_dd,
            turtle_trades, turtle_ret, turtle_sh, turtle_dd, winner_sym, t0.elapsed().as_secs_f64());

        records.push(WfRec {
            wi,
            period,
            macd_trades,
            macd_return: macd_ret,
            macd_sharpe: macd_sh,
            macd_dd,
            macd_passed,
            turtle_trades,
            turtle_return: turtle_ret,
            turtle_sharpe: turtle_sh,
            turtle_dd,
            turtle_passed,
        });
    }

    // ── Summary ─────────────────────────────────────────────────────────────────
    let tot = records.len();
    let macd_pass = records.iter().filter(|r| r.macd_passed).count();
    let turtle_pass = records.iter().filter(|r| r.turtle_passed).count();
    let avg_macd = if tot > 0 {
        records.iter().map(|r| r.macd_return).sum::<f64>() / tot as f64
    } else {
        0.0
    };
    let avg_turtle = if tot > 0 {
        records.iter().map(|r| r.turtle_return).sum::<f64>() / tot as f64
    } else {
        0.0
    };
    let worst_macd = records
        .iter()
        .map(|r| r.macd_return)
        .fold(0.0_f64, |a, v| a.min(v));
    let worst_turtle = records
        .iter()
        .map(|r| r.turtle_return)
        .fold(0.0_f64, |a, v| a.min(v));
    let hth_turtle = records
        .iter()
        .filter(|r| r.turtle_return > r.macd_return)
        .count();
    let hth_macd = records
        .iter()
        .filter(|r| r.macd_return > r.turtle_return)
        .count();

    println!("\n═══ SUMMARY: MACD+Regime ═══");
    println!(
        "  {}/{} windows passed ({:.0}% fail)",
        macd_pass,
        tot,
        if tot > 0 {
            (tot - macd_pass) as f64 / tot as f64 * 100.0
        } else {
            0.0
        }
    );
    println!("  Avg OOS: {:+.1}% | Worst: {:.1}%", avg_macd, worst_macd);

    println!("\n═══ SUMMARY: Turtle+MACD ═══");
    println!(
        "  {}/{} windows passed ({:.0}% fail)",
        turtle_pass,
        tot,
        if tot > 0 {
            (tot - turtle_pass) as f64 / tot as f64 * 100.0
        } else {
            0.0
        }
    );
    println!(
        "  Avg OOS: {:+.1}% | Worst: {:.1}%",
        avg_turtle, worst_turtle
    );

    println!("\n═══ HEAD-TO-HEAD ═══");
    println!("  Turtle+MACD wins: {}/{} windows", hth_turtle, tot);
    println!("  MACD+Regime wins: {}/{} windows", hth_macd, tot);

    // Comparison with Base5 results
    println!("\n═══ vs Base5 Walk-Forward (same design) ═══");
    println!(
        "  MACD+Regime: Base5 3/7 pass | OldGuardNoBNB {}/{} pass",
        macd_pass, tot
    );
    println!(
        "  Turtle+MACD: Base5 5/7 pass | OldGuardNoBNB {}/{} pass",
        turtle_pass, tot
    );

    let verdict = if turtle_pass > macd_pass {
        "Turtle+MACD is more robust on OldGuardNoBNB"
    } else if macd_pass > turtle_pass {
        "MACD+Regime is more robust on OldGuardNoBNB"
    } else {
        "Both equally robust on OldGuardNoBNB"
    };
    println!("\n  → {}", verdict);

    write_snapshots(&records)?;
    Ok(())
}

fn write_snapshots(recs: &[WfRec]) -> anyhow::Result<()> {
    std::fs::create_dir_all("snapshots")?;
    let cp = PathBuf::from("snapshots/oldguard_walk_forward_stress_latest.csv");
    let mp = PathBuf::from("snapshots/oldguard_walk_forward_stress_latest.md");

    let mut csv = String::from("window,period,macd_trades,macd_return,macd_sharpe,macd_dd,macd_passed,turtle_trades,turtle_return,turtle_sharpe,turtle_dd,turtle_passed,winner\n");
    for r in recs {
        csv.push_str(&format!(
            "{},{},{},{:.1},{},{:.1},{},{},{:.1},{},{:.1},{},{}\n",
            r.wi,
            r.period,
            r.macd_trades,
            r.macd_return,
            r.macd_sharpe,
            r.macd_dd,
            r.macd_passed,
            r.turtle_trades,
            r.turtle_return,
            r.turtle_sharpe,
            r.turtle_dd,
            r.turtle_passed,
            r.winner()
        ));
    }
    std::fs::write(&cp, &csv)?;

    let tot = recs.len();
    let macd_pass = recs.iter().filter(|r| r.macd_passed).count();
    let turtle_pass = recs.iter().filter(|r| r.turtle_passed).count();
    let avg_macd = if tot > 0 {
        recs.iter().map(|r| r.macd_return).sum::<f64>() / tot as f64
    } else {
        0.0
    };
    let avg_turtle = if tot > 0 {
        recs.iter().map(|r| r.turtle_return).sum::<f64>() / tot as f64
    } else {
        0.0
    };
    let worst_macd = recs
        .iter()
        .map(|r| r.macd_return)
        .fold(0.0_f64, |a, v| a.min(v));
    let worst_turtle = recs
        .iter()
        .map(|r| r.turtle_return)
        .fold(0.0_f64, |a, v| a.min(v));
    let hth_turtle = recs
        .iter()
        .filter(|r| r.turtle_return > r.macd_return)
        .count();
    let hth_macd = recs
        .iter()
        .filter(|r| r.macd_return > r.turtle_return)
        .count();

    let mut md = format!(
        "# Walk-Forward Stress: OldGuardNoBNB\n\n\
        ## Summary\n\n\
        - Universe: OldGuardNoBNB (BTC, ETH, XRP, LTC, EOS, BCH)\n\
        - Design: {}b train / {}b test / {}b hold / 0.1% taker\n\
        - Ranking: MACD gap strength (same for both strategies)\n\n\
        ## MACD+Regime\n\n\
        - Windows: {}/{} passed ({:.0}% fail)\n\
        - Avg OOS return: {:+.1}% | Worst OOS: {:.1}%\n\n\
        ## Turtle+MACD\n\n\
        - Windows: {}/{} passed ({:.0}% fail)\n\
        - Avg OOS return: {:+.1}% | Worst OOS: {:.1}%\n\n\
        ## Head-to-Head\n\n\
        - Turtle+MACD wins: {}/{} windows\n\
        - MACD+Regime wins: {}/{} windows\n\n\
        ## vs Base5 (same design)\n\n\
        | Strategy | Base5 Pass | OldGuardNoBNB Pass |\n\
        |----------|-----------|-------------------|\n\
        | MACD+Regime | 3/7 | {}/{} |\n\
        | Turtle+MACD | 5/7 | {}/{} |\n\n\
        ## Per-Window Results\n\n\
        | Window | Period | MR Ret% | MR Pass | TM Ret% | TM Pass | Winner |\n\
        |--------|--------|---------|---------|---------|---------|--------|\n",
        TRAIN_BARS,
        TEST_BARS,
        HOLD_BARS,
        macd_pass,
        tot,
        if tot > 0 {
            (tot - macd_pass) as f64 / tot as f64 * 100.0
        } else {
            0.0
        },
        avg_macd,
        worst_macd,
        turtle_pass,
        tot,
        if tot > 0 {
            (tot - turtle_pass) as f64 / tot as f64 * 100.0
        } else {
            0.0
        },
        avg_turtle,
        worst_turtle,
        hth_turtle,
        tot,
        hth_macd,
        tot,
        macd_pass,
        tot,
        turtle_pass,
        tot
    );

    for r in recs {
        md.push_str(&format!(
            "| {:02} | {} | {:+.1}% | {} | {:+.1}% | {} | {} |\n",
            r.wi,
            r.period,
            r.macd_return,
            if r.macd_passed { "✅" } else { "❌" },
            r.turtle_return,
            if r.turtle_passed { "✅" } else { "❌" },
            r.winner()
        ));
    }

    md.push_str("\n## Interpretation\n\n");
    md.push_str("- *Add honest interpretation after running.*\n");

    std::fs::write(&mp, &md)?;
    println!("\nSnapshots: {} {}", cp.display(), mp.display());
    Ok(())
}
