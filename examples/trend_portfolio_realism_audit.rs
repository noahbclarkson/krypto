//! Portfolio-construction realism audit for the daily trend benchmark leaders.
//!
//! Goal: keep the same fair signal / fee / hold assumptions as the head-to-head harness,
//! but evaluate the leaders on a time-aligned portfolio basis instead of mostly aggregated
//! trade totals.
//!
//! We audit:
//! - aligned equal-weight portfolio return / Sharpe / max drawdown
//! - average and peak concurrent positions across the basket
//! - percent of days with idle capital
//! - gap between summed trade-return headline and time-aligned portfolio outcome

use anyhow::Result;
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use polars::prelude::*;

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
];
const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const WARMUP_BARS: usize = 200;
const PERIOD: usize = 20;

#[derive(Clone, Copy, Debug)]
enum StrategyKind {
    Macd,
    MacdRegime,
    TurtleRegime,
    TurtleRegimeMacd,
}

impl StrategyKind {
    fn name(&self) -> &'static str {
        match self {
            Self::Macd => "MACD",
            Self::MacdRegime => "MACD+Regime",
            Self::TurtleRegime => "Turtle+Regime",
            Self::TurtleRegimeMacd => "Turtle+Regime+MACD",
        }
    }

    fn all() -> &'static [StrategyKind] {
        &[
            Self::Macd,
            Self::MacdRegime,
            Self::TurtleRegime,
            Self::TurtleRegimeMacd,
        ]
    }
}

#[derive(Default, Clone, Debug)]
struct SymbolAudit {
    equity_curve: Vec<f64>,
    active_curve: Vec<bool>,
    trade_return_sum_pct: f64,
    trades: usize,
    wins: usize,
}

#[derive(Clone, Debug)]
struct AuditRow {
    strategy: &'static str,
    aligned_return_pct: f64,
    aligned_sharpe: f64,
    aligned_max_dd_pct: f64,
    trade_sum_return_pct: f64,
    trade_sum_gap_pct: f64,
    trades: usize,
    win_rate_pct: f64,
    avg_active_positions: f64,
    peak_active_positions: usize,
    avg_capital_usage_pct: f64,
    idle_days_pct: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== TREND PORTFOLIO REALISM AUDIT ===\n");
    println!("Universe: {}", SYMBOLS.join(", "));
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side", TAKER_FEE * 100.0);
    println!("Audit lens: time-aligned equal-weight portfolio + capital occupancy\n");

    let loader = DataLoader::new(None, None);
    let mut data = Vec::new();
    for &symbol in SYMBOLS {
        print!("Loading {}... ", symbol);
        let raw = loader.fetch_with_cache(symbol, "1d", CANDLES).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        println!("{} bars", df.height());
        data.push((symbol, df));
    }

    let min_len = data.iter().map(|(_, df)| df.height()).min().unwrap_or(0);
    let steps = min_len.saturating_sub(1);
    if steps == 0 {
        anyhow::bail!("not enough data");
    }

    let mut rows = Vec::new();
    for &strategy in StrategyKind::all() {
        let mut symbol_audits = Vec::new();
        for (_, df) in &data {
            let truncated = df.slice(0, min_len);
            symbol_audits.push(run_symbol_audit(&truncated, strategy)?);
        }
        rows.push(summarize(strategy, &symbol_audits, steps));
    }

    rows.sort_by(|a, b| {
        b.aligned_return_pct
            .partial_cmp(&a.aligned_return_pct)
            .unwrap()
    });

    println!(
        "{:<20} {:>10} {:>8} {:>8} {:>10} {:>9} {:>8} {:>9} {:>8} {:>8} {:>8}",
        "Strategy",
        "Aligned",
        "Sharpe",
        "MaxDD",
        "TradeSum",
        "Gap",
        "Trades",
        "WinRate",
        "AvgAct",
        "Peak",
        "Idle%"
    );
    println!("{}", "-".repeat(120));
    for row in &rows {
        println!(
            "{:<20} {:>9.1}% {:>8.2} {:>7.1}% {:>9.1}% {:>+8.1}% {:>8} {:>8.1}% {:>8.2} {:>8} {:>7.1}%",
            row.strategy,
            row.aligned_return_pct,
            row.aligned_sharpe,
            row.aligned_max_dd_pct,
            row.trade_sum_return_pct,
            row.trade_sum_gap_pct,
            row.trades,
            row.win_rate_pct,
            row.avg_active_positions,
            row.peak_active_positions,
            row.idle_days_pct,
        );
    }

    println!("\nNotes:");
    println!("- Aligned = equal-weight portfolio return from time-synced open-to-open mark-to-market across symbols.");
    println!(
        "- TradeSum = simple sum of per-trade % returns across symbols (the usual headline style)."
    );
    println!("- Gap = aligned return - trade-sum return; big negative gaps mean the headline is flattering the strategy.");
    println!(
        "- AvgAct / Peak / Idle% show how much capital is actually deployed across the basket."
    );

    Ok(())
}

fn summarize(strategy: StrategyKind, audits: &[SymbolAudit], steps: usize) -> AuditRow {
    let mut portfolio_curve = vec![0.0; steps + 1];
    let mut active_counts = vec![0usize; steps + 1];
    let mut trade_sum_return_pct = 0.0;
    let mut trades = 0usize;
    let mut wins = 0usize;

    for audit in audits {
        trade_sum_return_pct += audit.trade_return_sum_pct;
        trades += audit.trades;
        wins += audit.wins;
        for i in 0..=steps {
            portfolio_curve[i] += audit.equity_curve[i];
            if audit.active_curve[i] {
                active_counts[i] += 1;
            }
        }
    }

    for value in &mut portfolio_curve {
        *value /= audits.len() as f64;
    }

    let aligned_return_pct = (portfolio_curve.last().copied().unwrap_or(1.0) - 1.0) * 100.0;
    let aligned_sharpe = calc_sharpe(&portfolio_curve);
    let aligned_max_dd_pct = calc_max_drawdown_pct(&portfolio_curve);
    let avg_active_positions =
        active_counts.iter().sum::<usize>() as f64 / active_counts.len() as f64;
    let peak_active_positions = active_counts.iter().copied().max().unwrap_or(0);
    let avg_capital_usage_pct = avg_active_positions / audits.len() as f64 * 100.0;
    let idle_days_pct = active_counts.iter().filter(|&&c| c == 0).count() as f64
        / active_counts.len() as f64
        * 100.0;
    let win_rate_pct = if trades == 0 {
        0.0
    } else {
        wins as f64 / trades as f64 * 100.0
    };

    AuditRow {
        strategy: strategy.name(),
        aligned_return_pct,
        aligned_sharpe,
        aligned_max_dd_pct,
        trade_sum_return_pct,
        trade_sum_gap_pct: aligned_return_pct - trade_sum_return_pct,
        trades,
        win_rate_pct,
        avg_active_positions,
        peak_active_positions,
        avg_capital_usage_pct,
        idle_days_pct,
    }
}

fn run_symbol_audit(df: &DataFrame, strategy: StrategyKind) -> Result<SymbolAudit> {
    let signals = generate_signals(df, strategy)?;
    let open = df.column("open")?.f64()?;
    let n = open.len();
    let steps = n.saturating_sub(1);
    let mut equity_curve = vec![1.0; steps + 1];
    let mut active_curve = vec![false; steps + 1];
    let mut equity = 1.0;
    let mut i = WARMUP_BARS;
    let mut trade_return_sum_pct = 0.0;
    let mut trades = 0usize;
    let mut wins = 0usize;

    while i + HOLD_BARS + 1 < n {
        let signal = signals.get(i).copied().unwrap_or(0);
        if signal == 0 {
            equity_curve[i + 1] = equity;
            i += 1;
            continue;
        }

        let entry_idx = i + 1;
        let exit_idx = i + 1 + HOLD_BARS;
        let entry = match open.get(entry_idx) {
            Some(v) if v > 0.0 => v,
            _ => {
                equity_curve[i + 1] = equity;
                i += 1;
                continue;
            }
        };

        equity *= 1.0 - TAKER_FEE;
        equity_curve[entry_idx] = equity;
        active_curve[entry_idx] = true;

        for t in entry_idx..exit_idx {
            let o0 = match open.get(t) {
                Some(v) if v > 0.0 => v,
                _ => continue,
            };
            let o1 = match open.get(t + 1) {
                Some(v) if v > 0.0 => v,
                _ => continue,
            };
            let daily_ret = if signal > 0 {
                o1 / o0 - 1.0
            } else {
                o0 / o1 - 1.0
            };
            equity *= 1.0 + daily_ret;
            equity_curve[t + 1] = equity;
            active_curve[t + 1] = true;
        }

        equity *= 1.0 - TAKER_FEE;
        equity_curve[exit_idx] = equity;

        let exit = open.get(exit_idx).unwrap_or(entry);
        let gross_trade = if signal > 0 {
            (exit / entry - 1.0) * 100.0
        } else {
            (entry / exit - 1.0) * 100.0
        };
        let net_trade = gross_trade - 2.0 * TAKER_FEE * 100.0;
        trade_return_sum_pct += net_trade;
        trades += 1;
        if net_trade > 0.0 {
            wins += 1;
        }

        i = exit_idx;
    }

    for idx in 1..=steps {
        if equity_curve[idx] == 1.0 && equity != 1.0 {
            equity_curve[idx] = equity_curve[idx - 1];
        } else if idx > 0 && equity_curve[idx] == 1.0 {
            equity_curve[idx] = equity_curve[idx - 1];
        }
    }

    Ok(SymbolAudit {
        equity_curve,
        active_curve,
        trade_return_sum_pct,
        trades,
        wins,
    })
}

fn generate_signals(df: &DataFrame, strategy: StrategyKind) -> Result<Vec<i32>> {
    match strategy {
        StrategyKind::Macd => generate_macd_signals(df),
        StrategyKind::MacdRegime => generate_macd_regime_signals(df),
        StrategyKind::TurtleRegime => generate_turtle_regime_signals(df, PERIOD),
        StrategyKind::TurtleRegimeMacd => generate_turtle_regime_macd_signals(df, PERIOD),
    }
}

fn generate_macd_signals(df: &DataFrame) -> Result<Vec<i32>> {
    let macd = df.column("macd").ok().and_then(|s| s.f64().ok());
    let signal = df.column("macd_signal").ok().and_then(|s| s.f64().ok());
    let close = df.column("close")?.f64()?;
    let n = close.len();
    let mut signals = vec![0i32; n];

    if let (Some(macd_series), Some(signal_series)) = (macd, signal) {
        for i in 1..n {
            let macd_curr = macd_series.get(i).unwrap_or(0.0);
            let sig_curr = signal_series.get(i).unwrap_or(0.0);
            if macd_curr > sig_curr {
                signals[i] = 1;
            } else if macd_curr < sig_curr {
                signals[i] = -1;
            }
        }
    }
    Ok(signals)
}

fn generate_macd_regime_signals(df: &DataFrame) -> Result<Vec<i32>> {
    let macd = generate_macd_signals(df)?;
    let close = df.column("close")?.f64()?;
    let sma_200 = calculate_sma(&close, 200);
    let mut out = vec![0i32; macd.len()];

    for i in 0..macd.len() {
        let sig = macd[i];
        let price = close.get(i).unwrap_or(0.0);
        let sma = sma_200.get(i).copied().unwrap_or(0.0);
        if sig > 0 && price > sma {
            out[i] = 1;
        } else if sig < 0 && price < sma {
            out[i] = -1;
        }
    }
    Ok(out)
}

fn generate_turtle_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let n = close.len();
    let mut signals = vec![0i32; n];

    for i in period..n {
        let period_high = (i - period..i)
            .filter_map(|j| high.get(j))
            .fold(f64::NEG_INFINITY, f64::max);
        let period_low = (i - period..i)
            .filter_map(|j| low.get(j))
            .fold(f64::INFINITY, f64::min);
        let current_close = close.get(i).unwrap_or(0.0);

        if current_close > period_high {
            signals[i] = 1;
        } else if current_close < period_low {
            signals[i] = -1;
        }
    }

    Ok(signals)
}

fn generate_turtle_regime_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let turtle = generate_turtle_signals(df, period)?;
    let close = df.column("close")?.f64()?;
    let sma_200 = calculate_sma(&close, 200);
    let mut out = vec![0i32; turtle.len()];

    for i in 0..turtle.len() {
        let sig = turtle[i];
        let price = close.get(i).unwrap_or(0.0);
        let sma = sma_200.get(i).copied().unwrap_or(0.0);
        if sig > 0 && price > sma {
            out[i] = 1;
        } else if sig < 0 {
            out[i] = -1;
        }
    }
    Ok(out)
}

fn generate_turtle_regime_macd_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let turtle = generate_turtle_regime_signals(df, period)?;
    let macd = generate_macd_signals(df)?;
    let mut out = vec![0i32; turtle.len()];

    for i in 0..turtle.len() {
        let t = turtle[i];
        let m = macd[i];
        if t > 0 && m > 0 {
            out[i] = 1;
        } else if t < 0 && m < 0 {
            out[i] = -1;
        }
    }
    Ok(out)
}

fn calculate_sma(values: &Float64Chunked, period: usize) -> Vec<f64> {
    let mut out = vec![0.0; values.len()];
    let mut sum = 0.0;
    for i in 0..values.len() {
        sum += values.get(i).unwrap_or(0.0);
        if i >= period {
            sum -= values.get(i - period).unwrap_or(0.0);
        }
        if i + 1 >= period {
            out[i] = sum / period as f64;
        }
    }
    out
}

fn calc_sharpe(equity_curve: &[f64]) -> f64 {
    if equity_curve.len() < 2 {
        return 0.0;
    }
    let returns: Vec<f64> = equity_curve.windows(2).map(|w| w[1] / w[0] - 1.0).collect();
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let var = returns
        .iter()
        .map(|r| {
            let d = r - mean;
            d * d
        })
        .sum::<f64>()
        / returns.len() as f64;
    let std = var.sqrt();
    if std <= 1e-12 {
        0.0
    } else {
        mean / std * 365.0f64.sqrt()
    }
}

fn calc_max_drawdown_pct(equity_curve: &[f64]) -> f64 {
    let mut peak = equity_curve.first().copied().unwrap_or(1.0);
    let mut max_dd = 0.0;
    for &value in equity_curve {
        if value > peak {
            peak = value;
        }
        let dd = (value / peak - 1.0) * 100.0;
        if dd < max_dd {
            max_dd = dd;
        }
    }
    max_dd.abs()
}
