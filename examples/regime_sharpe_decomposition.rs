//! T64: Regime Sharpe Decomposition
//!
//! Trust-lab attribution for the current Turtle-only live path:
//!   entry: Turtle breakout (close > max close over EP bars)
//!   exit:  Turtle ATR trailing stop + HOLD_MAX
//!   gate:  BTC ATR_RANK(AP=17, LB=42, T=5)
//!
//! Outputs regime-conditional daily Sharpe using calendar-day equity returns
//! including flat/cash days. Also runs a T=0 no-gate control so the production
//! ATR_RANK=5 filter can be judged by regime instead of another blind hyperopt.
//!
//! Exports:
//!   snapshots/regime_sharpe_decomposition.csv
//!   snapshots/regime_sharpe_decomposition.md

use anyhow::Result;
use chrono::Utc;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

// Production params (from src/live/config.rs / T59)
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const HOLD_MAX: usize = 12;
const POSITION_CAP: usize = 3;
const VOL_LOOKBACK: usize = 96;
const TAKER_FEE: f64 = 0.001;

const REGIME_ATR_PERIOD: usize = 17;
const REGIME_LOOKBACK: usize = 42;
const PROD_ATR_RANK_THRESHOLD: f64 = 5.0;

const CANDLES: u32 = 3000;
const WARMUP_BARS: usize = 300;
const BTC_RET_LOOKBACK: usize = 21;
const BTC_VOL_LOOKBACK: usize = 21;

const BASE_SYMBOLS: [&str; 6] = [
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
];

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

#[derive(Clone)]
struct Trade {
    entry_bar: usize,
}

struct SimResult {
    name: String,
    threshold: f64,
    final_equity: f64,
    max_dd: f64,
    trades: Vec<Trade>,
    daily_returns: Vec<f64>,
}

#[derive(Default, Clone)]
struct BucketStats {
    days: usize,
    nonzero_days: usize,
    trades: usize,
    returns: Vec<f64>,
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window {
        return 0.0;
    }
    vals[idx + 1 - window..=idx].iter().sum::<f64>() / window as f64
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period {
        return 0.0;
    }
    let mut total = 0.0;
    for i in (idx + 1 - period)..=idx {
        let h = high[i];
        let l = low[i];
        let c0 = close[i.saturating_sub(1)];
        total += (h - l).max((h - c0).abs()).max((l - c0).abs());
    }
    total / period as f64
}

fn btc_atr_pct(btc_data: &SymData, period: usize, lookback: usize, idx: usize) -> f64 {
    if idx < lookback + period {
        return 50.0;
    }
    let curr = atr_at(&btc_data.high, &btc_data.low, &btc_data.close, period, idx);
    if curr <= 0.0 {
        return 50.0;
    }
    let start = idx + 1 - lookback - period;
    let end = idx + 1 - period;
    if end <= start {
        return 50.0;
    }
    let mut hist: Vec<f64> = (start..=end)
        .map(|i| atr_at(&btc_data.high, &btc_data.low, &btc_data.close, period, i))
        .filter(|x| x.is_finite() && *x > 0.0)
        .collect();
    if hist.is_empty() {
        return 50.0;
    }
    hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let count = hist.iter().filter(|&&x| x < curr).count();
    (count as f64 / hist.len() as f64) * 100.0
}

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], idx: usize) -> bool {
    if idx < TURTLE_ENTRY {
        return false;
    }
    let start = idx - TURTLE_ENTRY;
    let max_close = close[start..idx]
        .iter()
        .fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    if close[idx] <= max_close {
        return false;
    }
    if ATR_ENTRY_MULT > 0.0 {
        let atr = atr_at(high, low, close, TURTLE_ATR_PERIOD, idx);
        if close[idx] < max_close + atr * ATR_ENTRY_MULT {
            return false;
        }
    }
    true
}

fn btc_return_21d(btc: &SymData, idx: usize) -> f64 {
    if idx < BTC_RET_LOOKBACK || idx >= btc.close.len() {
        return 0.0;
    }
    btc.close[idx] / btc.close[idx - BTC_RET_LOOKBACK] - 1.0
}

fn btc_realized_vol_21d(btc: &SymData, idx: usize) -> f64 {
    if idx < BTC_VOL_LOOKBACK + 1 || idx >= btc.close.len() {
        return 0.0;
    }
    let mut rets = Vec::with_capacity(BTC_VOL_LOOKBACK);
    for i in (idx + 1 - BTC_VOL_LOOKBACK)..=idx {
        if i == 0 {
            continue;
        }
        rets.push(btc.close[i] / btc.close[i - 1] - 1.0);
    }
    if rets.len() < 2 {
        return 0.0;
    }
    let mean = rets.iter().sum::<f64>() / rets.len() as f64;
    (rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / rets.len() as f64).sqrt()
}

fn percentile(mut vals: Vec<f64>, p: f64) -> f64 {
    vals.retain(|x| x.is_finite());
    if vals.is_empty() {
        return 0.0;
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = ((vals.len() - 1) as f64 * p).round() as usize;
    vals[idx.min(vals.len() - 1)]
}

fn annualised_sharpe(rets: &[f64]) -> f64 {
    if rets.len() < 10 {
        return 0.0;
    }
    let mean = rets.iter().sum::<f64>() / rets.len() as f64;
    let var = rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / rets.len() as f64;
    if var <= 0.0 {
        return 0.0;
    }
    mean / var.sqrt() * 365.0_f64.sqrt()
}

fn compound_equity(rets: &[f64]) -> Vec<f64> {
    let mut eq = 1.0_f64;
    let mut out = Vec::with_capacity(rets.len());
    for &r in rets {
        eq *= 1.0 + r;
        out.push(eq);
    }
    out
}

fn max_dd_from_equity(eq: &[f64]) -> f64 {
    let mut peak = 1.0_f64;
    let mut max_dd = 0.0_f64;
    for &e in eq {
        if e > peak {
            peak = e;
        }
        let dd = 1.0 - e / peak;
        if dd > max_dd {
            max_dd = dd;
        }
    }
    max_dd
}

fn simulate(
    name: &str,
    atr_rank_threshold: f64,
    sym_data: &HashMap<String, SymData>,
    btc_data: &SymData,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
) -> SimResult {
    let mut daily_returns = vec![0.0_f64; test_end - test_start];
    let mut trades = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let btc_pct = btc_atr_pct(btc_data, REGIME_ATR_PERIOD, REGIME_LOOKBACK, bar);
        if btc_pct < atr_rank_threshold {
            bar += 1;
            continue;
        }

        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() {
                    continue;
                }
                let dv = rolling_avg(&sd.vol, VOL_LOOKBACK, bar) * sd.close[bar];
                scores.push((
                    sym.as_str(),
                    if dv.is_finite() && dv > 0.0 { dv } else { 0.0 },
                ));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores
            .into_iter()
            .take(POSITION_CAP)
            .map(|(s, _)| s.to_string())
            .collect();

        let mut entered = false;
        for sym in &top_syms {
            let Some(sd) = sym_data.get(sym) else {
                continue;
            };
            if bar < TURTLE_ENTRY + 1
                || bar >= sd.close.len()
                || !turtle_signal(&sd.close, &sd.high, &sd.low, bar)
            {
                continue;
            }

            let entry = sd.close[bar] * (1.0 + TAKER_FEE);
            let entry_bar_next = bar + 1;
            let n = sd.close.len();
            let mut highest_high = sd.high[entry_bar_next];
            let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
            let mut exit_bar = max_bar;

            let warm_start = entry_bar_next.saturating_sub(TURTLE_ATR_PERIOD);
            let mut atr_buf: std::collections::VecDeque<f64> =
                std::collections::VecDeque::with_capacity(TURTLE_ATR_PERIOD);
            for b in warm_start..entry_bar_next {
                if b > 0 {
                    let c0 = sd.close[b.saturating_sub(1)];
                    let tr = (sd.high[b] - sd.low[b])
                        .max((sd.high[b] - c0).abs())
                        .max((sd.low[b] - c0).abs());
                    atr_buf.push_back(tr);
                }
            }

            for b in entry_bar_next..=max_bar {
                if sd.high[b] > highest_high {
                    highest_high = sd.high[b];
                }
                let c0 = sd.close[b.saturating_sub(1)];
                let tr = (sd.high[b] - sd.low[b])
                    .max((sd.high[b] - c0).abs())
                    .max((sd.low[b] - c0).abs());
                atr_buf.push_back(tr);
                if atr_buf.len() > TURTLE_ATR_PERIOD {
                    atr_buf.pop_front();
                }

                if atr_buf.len() == TURTLE_ATR_PERIOD {
                    let atr = atr_buf.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
                    let turtle_stop = highest_high - TURTLE_ATR_MULT * atr;
                    if sd.low[b] <= turtle_stop {
                        exit_bar = b;
                        break;
                    }
                }

                if b >= entry_bar_next + HOLD_MAX {
                    exit_bar = b;
                    break;
                }
            }

            if let Some(&exit_px) = sd.close.get(exit_bar) {
                let exit = exit_px * (1.0 - TAKER_FEE);
                let pct_ret = exit / entry - 1.0;
                // Allocate each trade geometrically across held calendar bars so final
                // compounded equity matches trade-level compounding, while regime Sharpe
                // still reflects exposure days instead of a single exit-day impulse.
                let held_days = (exit_bar.saturating_sub(entry_bar_next)).max(1);
                let daily_r = (1.0 + pct_ret).powf(1.0 / held_days as f64) - 1.0;
                let start_day = entry_bar_next.saturating_sub(test_start);
                let end_day = (start_day + held_days).min(daily_returns.len());
                for r in daily_returns.iter_mut().take(end_day).skip(start_day) {
                    *r = (1.0 + *r) * (1.0 + daily_r) - 1.0;
                }
                trades.push(Trade { entry_bar: bar });
                bar = exit_bar;
                entered = true;
                break;
            }
        }

        if !entered {
            bar += 1;
        }
    }

    let eq_curve = compound_equity(&daily_returns);
    let final_equity = *eq_curve.last().unwrap_or(&1.0);
    let max_dd = max_dd_from_equity(&eq_curve);
    SimResult {
        name: name.to_string(),
        threshold: atr_rank_threshold,
        final_equity,
        max_dd,
        trades,
        daily_returns,
    }
}

fn add_return_bucket(map: &mut HashMap<String, BucketStats>, name: &str, r: f64) {
    let b = map.entry(name.to_string()).or_default();
    b.days += 1;
    if r.abs() > 1e-12 {
        b.nonzero_days += 1;
    }
    b.returns.push(r);
}

fn add_trade_bucket(map: &mut HashMap<String, BucketStats>, name: &str) {
    map.entry(name.to_string()).or_default().trades += 1;
}

fn decompose(
    sim: &SimResult,
    btc: &SymData,
    test_start: usize,
    test_end: usize,
    q25: f64,
    q75: f64,
) -> HashMap<String, BucketStats> {
    let mut buckets: HashMap<String, BucketStats> = HashMap::new();

    for (i, &r) in sim.daily_returns.iter().enumerate() {
        let idx = test_start + i;
        if idx >= test_end || idx >= btc.close.len() {
            break;
        }
        let btc_ret = btc_return_21d(btc, idx);
        let vol = btc_realized_vol_21d(btc, idx);

        if btc_ret > 0.0 {
            add_return_bucket(&mut buckets, "bull_21d", r);
        }
        if btc_ret < 0.0 {
            add_return_bucket(&mut buckets, "bear_21d", r);
        }
        if vol <= q25 {
            add_return_bucket(&mut buckets, "chop_vol_q1", r);
        }
        if vol >= q75 {
            add_return_bucket(&mut buckets, "trend_vol_q4", r);
        }
        add_return_bucket(&mut buckets, "all_days", r);
    }

    for tr in &sim.trades {
        let btc_ret = btc_return_21d(btc, tr.entry_bar);
        let vol = btc_realized_vol_21d(btc, tr.entry_bar);
        if btc_ret > 0.0 {
            add_trade_bucket(&mut buckets, "bull_21d");
        }
        if btc_ret < 0.0 {
            add_trade_bucket(&mut buckets, "bear_21d");
        }
        if vol <= q25 {
            add_trade_bucket(&mut buckets, "chop_vol_q1");
        }
        if vol >= q75 {
            add_trade_bucket(&mut buckets, "trend_vol_q4");
        }
        add_trade_bucket(&mut buckets, "all_days");
    }

    buckets
}

fn bucket_equity(rets: &[f64]) -> f64 {
    rets.iter().fold(1.0_f64, |eq, r| eq * (1.0 + r))
}

fn fmt_pct(x: f64) -> String {
    format!("{:.1}%", x * 100.0)
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== T64: REGIME SHARPE DECOMPOSITION ===");
    println!(
        "Production: EP={}, ATR({},{}), HM={}, CAP={}, VL={}, ATR_RANK(AP={}, LB={}, T={})",
        TURTLE_ENTRY,
        TURTLE_ATR_PERIOD,
        TURTLE_ATR_MULT,
        HOLD_MAX,
        POSITION_CAP,
        VOL_LOOKBACK,
        REGIME_ATR_PERIOD,
        REGIME_LOOKBACK,
        PROD_ATR_RANK_THRESHOLD
    );
    println!("Control: same strategy with ATR_RANK T=0 (no entry gate)\n");

    let loader = DataLoader::new(None, None);
    let mut sym_data = HashMap::new();
    let mut min_len = usize::MAX;

    for &sym in &BASE_SYMBOLS {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close = df
            .column("close")?
            .f64()?
            .into_no_null_iter()
            .collect::<Vec<_>>();
        let high = df
            .column("high")?
            .f64()?
            .into_no_null_iter()
            .collect::<Vec<_>>();
        let low = df
            .column("low")?
            .f64()?
            .into_no_null_iter()
            .collect::<Vec<_>>();
        let vol = df
            .column("volume")?
            .f64()?
            .into_no_null_iter()
            .collect::<Vec<_>>();
        min_len = min_len.min(close.len());
        sym_data.insert(
            sym.to_string(),
            SymData {
                close,
                high,
                low,
                vol,
            },
        );
    }

    let btc = sym_data.get("BTCUSDT").expect("BTCUSDT loaded");
    let symbols: Vec<String> = BASE_SYMBOLS.iter().map(|&s| s.to_string()).collect();
    let test_start = WARMUP_BARS;
    let test_end = min_len;

    let vols: Vec<f64> = (test_start..test_end)
        .map(|idx| btc_realized_vol_21d(btc, idx))
        .collect();
    let q25 = percentile(vols.clone(), 0.25);
    let q75 = percentile(vols, 0.75);

    let prod = simulate(
        "prod_atr_rank_5",
        PROD_ATR_RANK_THRESHOLD,
        &sym_data,
        btc,
        &symbols,
        test_start,
        test_end,
    );
    let no_gate = simulate(
        "no_gate_t0",
        0.0,
        &sym_data,
        btc,
        &symbols,
        test_start,
        test_end,
    );
    let sims = [&prod, &no_gate];

    let order = [
        "all_days",
        "bull_21d",
        "bear_21d",
        "chop_vol_q1",
        "trend_vol_q4",
    ];

    let mut csv = String::from(
        "strategy,threshold,regime,days,nonzero_days,trades,equity,return_pct,sharpe\n",
    );
    for sim in sims {
        let buckets = decompose(sim, btc, test_start, test_end, q25, q75);
        println!("--- {} (T={:.1}) ---", sim.name, sim.threshold);
        println!(
            "Final equity {:.2}x | Sharpe {:.2} | MaxDD {} | Trades {}",
            sim.final_equity,
            annualised_sharpe(&sim.daily_returns),
            fmt_pct(sim.max_dd),
            sim.trades.len()
        );
        println!(
            "{:<14} {:>6} {:>8} {:>7} {:>10} {:>10}",
            "Regime", "Days", "NZDays", "Trades", "Equity", "Sharpe"
        );
        for regime in order {
            let b = buckets.get(regime).cloned().unwrap_or_default();
            let eq = bucket_equity(&b.returns);
            let sh = annualised_sharpe(&b.returns);
            println!(
                "{:<14} {:>6} {:>8} {:>7} {:>9.2}x {:>10.2}",
                regime, b.days, b.nonzero_days, b.trades, eq, sh
            );
            csv.push_str(&format!(
                "{},{:.1},{},{},{},{},{:.6},{:.3},{:.6}\n",
                sim.name,
                sim.threshold,
                regime,
                b.days,
                b.nonzero_days,
                b.trades,
                eq,
                (eq - 1.0) * 100.0,
                sh
            ));
        }
        println!();
    }

    std::fs::create_dir_all("snapshots")?;
    File::create("snapshots/regime_sharpe_decomposition.csv")?.write_all(csv.as_bytes())?;

    let prod_b = decompose(&prod, btc, test_start, test_end, q25, q75);
    let no_b = decompose(&no_gate, btc, test_start, test_end, q25, q75);

    let mut md = File::create("snapshots/regime_sharpe_decomposition.md")?;
    writeln!(md, "# T64: Regime Sharpe Decomposition\n")?;
    writeln!(
        md,
        "Generated: {}\n",
        Utc::now().format("%Y-%m-%d %H:%M UTC")
    )?;
    writeln!(md, "## Method\n")?;
    writeln!(
        md,
        "- Universe: Base5 production symbols: `{}`",
        BASE_SYMBOLS.join(", ")
    )?;
    writeln!(md, "- Strategy: Turtle-only live path, EP={}, ATR({},{:.2}), HOLD_MAX={}, CAP={}, VL={}, fee={:.1}bps/side", TURTLE_ENTRY, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, HOLD_MAX, POSITION_CAP, VOL_LOOKBACK, TAKER_FEE * 10000.0)?;
    writeln!(
        md,
        "- Production gate: BTC ATR_RANK(AP={}, LB={}, T={:.1}); control uses T=0 no gate",
        REGIME_ATR_PERIOD, REGIME_LOOKBACK, PROD_ATR_RANK_THRESHOLD
    )?;
    writeln!(md, "- Daily Sharpe uses calendar-day returns including flat/cash days; trade PnL is allocated geometrically across held bars for attribution.")?;
    writeln!(md, "- This attribution curve is not a replacement for T59's exact event-compounded headline (112.27x / Sharpe 3.14); small equity differences can occur because T64 spreads trade PnL across held days to classify regimes.")?;
    writeln!(md, "- Bull/bear: BTC 21d return >/< 0. Chop/trend: BTC 21d realised vol bottom/top quartile over the test period (q25={:.4}, q75={:.4}).\n", q25, q75)?;

    writeln!(md, "## Headline\n")?;
    writeln!(md, "| Strategy | Equity | Sharpe | MaxDD | Trades |")?;
    writeln!(md, "|----------|--------|--------|-------|--------|")?;
    for sim in sims {
        writeln!(
            md,
            "| {} | {:.2}x | {:.2} | {} | {} |",
            sim.name,
            sim.final_equity,
            annualised_sharpe(&sim.daily_returns),
            fmt_pct(sim.max_dd),
            sim.trades.len()
        )?;
    }

    for (title, buckets) in [
        ("Production ATR_RANK=5", prod_b),
        ("No-gate control T=0", no_b),
    ] {
        writeln!(md, "\n## {}\n", title)?;
        writeln!(
            md,
            "| Regime | Days | Nonzero days | Trades | Equity | Sharpe |"
        )?;
        writeln!(
            md,
            "|--------|------|--------------|--------|--------|--------|"
        )?;
        for regime in order {
            let b = buckets.get(regime).cloned().unwrap_or_default();
            let eq = bucket_equity(&b.returns);
            writeln!(
                md,
                "| {} | {} | {} | {} | {:.2}x | {:.2} |",
                regime,
                b.days,
                b.nonzero_days,
                b.trades,
                eq,
                annualised_sharpe(&b.returns)
            )?;
        }
    }

    let prod_all = decompose(&prod, btc, test_start, test_end, q25, q75);
    let ng_all = decompose(&no_gate, btc, test_start, test_end, q25, q75);
    let prod_bear = prod_all
        .get("bear_21d")
        .map(|b| annualised_sharpe(&b.returns))
        .unwrap_or(0.0);
    let prod_bull = prod_all
        .get("bull_21d")
        .map(|b| annualised_sharpe(&b.returns))
        .unwrap_or(0.0);
    let prod_chop = prod_all
        .get("chop_vol_q1")
        .map(|b| annualised_sharpe(&b.returns))
        .unwrap_or(0.0);
    let prod_trend = prod_all
        .get("trend_vol_q4")
        .map(|b| annualised_sharpe(&b.returns))
        .unwrap_or(0.0);
    let ng_bear = ng_all
        .get("bear_21d")
        .map(|b| annualised_sharpe(&b.returns))
        .unwrap_or(0.0);
    let ng_bull = ng_all
        .get("bull_21d")
        .map(|b| annualised_sharpe(&b.returns))
        .unwrap_or(0.0);

    writeln!(md, "\n## Interpretation\n")?;
    writeln!(md, "- Production regime asymmetry: bull Sharpe {:.2}, bear Sharpe {:.2}, chop Sharpe {:.2}, trend-vol Sharpe {:.2}.", prod_bull, prod_bear, prod_chop, prod_trend)?;
    writeln!(md, "- ATR_RANK=5 vs no gate: full equity {:.2}x vs {:.2}x; full Sharpe {:.2} vs {:.2}; bear Sharpe {:.2} vs {:.2}; bull Sharpe {:.2} vs {:.2}.",
        prod.final_equity, no_gate.final_equity, annualised_sharpe(&prod.daily_returns), annualised_sharpe(&no_gate.daily_returns), prod_bear, ng_bear, prod_bull, ng_bull)?;
    writeln!(md, "- This is attribution, not a new hyperopt. T=5 remains production because high-threshold ATR_RANK variants already failed held-out validation; this control checks whether the tiny low-vol gate is materially helping or just reducing sample size.")?;

    println!("CSV: snapshots/regime_sharpe_decomposition.csv");
    println!("Report: snapshots/regime_sharpe_decomposition.md");
    Ok(())
}
