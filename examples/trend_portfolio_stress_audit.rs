//! Stress-test portfolio-construction realism for the current daily trend leaders.
//!
//! Extends the base `trend_portfolio_realism_audit` in two ways:
//! 1. evaluates the aligned portfolio lens across harsher / legacy-skewed universes
//! 2. adds a position-capped book so we can see whether leaders survive realistic capital concentration
//!
//! Execution assumptions stay matched to the fair daily harness:
//! - signal at close using only current/past data
//! - entry at next open
//! - exit at open after fixed 21-bar hold
//! - 0.1% taker fee on entry and exit

use anyhow::Result;
use chrono::Utc;
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use polars::prelude::*;
use std::{collections::HashMap, fs};

const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const WARMUP_BARS: usize = 200;
const PERIOD: usize = 20;
const POSITION_CAP: usize = 3;
const SNAPSHOT_DIR: &str = "snapshots";
const SNAPSHOT_LATEST_MD: &str = "snapshots/trend_portfolio_stress_latest.md";
const SNAPSHOT_LATEST_CSV: &str = "snapshots/trend_portfolio_stress_latest.csv";

const STRESS_UNIVERSES: &[(&str, &[&str])] = &[
    (
        "Base6",
        &[
            "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
        ],
    ),
    (
        "NoDOGE",
        &[
            "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT", "BNBUSDT",
        ],
    ),
    (
        "Legacy5",
        &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT"],
    ),
    ("LegacyCore4", &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT"]),
    (
        "LargeCaps6",
        &[
            "BTCUSDT", "ETHUSDT", "XRPUSDT", "ADAUSDT", "BNBUSDT", "LTCUSDT",
        ],
    ),
    (
        "OldGuard6",
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
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

#[derive(Clone, Debug)]
struct TradeWindow {
    entry_idx: usize,
    exit_idx: usize,
    strength: f64,
    gross_return: f64,
    net_return: f64,
}

#[derive(Clone, Debug)]
struct SymbolPlan {
    trades: Vec<TradeWindow>,
}

#[derive(Clone, Debug)]
struct PortfolioStats {
    aligned_return_pct: f64,
    sharpe: f64,
    max_dd_pct: f64,
    avg_active_positions: f64,
    idle_days_pct: f64,
    trade_sum_return_pct: f64,
    trades: usize,
    win_rate_pct: f64,
}

#[derive(Clone, Debug)]
struct SummaryRow {
    strategy: StrategyKind,
    uncapped: PortfolioStats,
    capped: PortfolioStats,
}

#[derive(Clone, Debug)]
struct UniverseSummary {
    label: String,
    symbols: Vec<String>,
    rows: Vec<SummaryRow>,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== TREND PORTFOLIO STRESS AUDIT ===\n");
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!("Fees: {:.1}% taker each side", TAKER_FEE * 100.0);
    println!(
        "Portfolio lenses: uncapped equal-weight and top-{} strength-capped book\n",
        POSITION_CAP
    );

    let loader = DataLoader::new(None, None);
    let mut all_symbols = Vec::<&str>::new();
    for &(_, symbols) in STRESS_UNIVERSES {
        for &symbol in symbols {
            if !all_symbols.contains(&symbol) {
                all_symbols.push(symbol);
            }
        }
    }

    let mut raw_cache = HashMap::<String, DataFrame>::new();
    for &symbol in &all_symbols {
        print!("Loading {}... ", symbol);
        let raw = loader.fetch_with_cache(symbol, "1d", CANDLES).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        println!("{} bars", df.height());
        raw_cache.insert(symbol.to_string(), df);
    }

    let mut universe_summaries = Vec::new();
    for &(label, symbols) in STRESS_UNIVERSES {
        println!("\n=== {} ===", label);
        println!("Symbols: {}", symbols.join(", "));

        let universe = aligned_universe(&raw_cache, symbols)?;
        let mut rows = Vec::new();
        for &strategy in StrategyKind::all() {
            let plans = build_symbol_plans(&universe, strategy)?;
            let uncapped = simulate_portfolio(&plans, universe.steps, None);
            let capped = simulate_portfolio(&plans, universe.steps, Some(POSITION_CAP));
            rows.push(SummaryRow {
                strategy,
                uncapped,
                capped,
            });
        }

        rows.sort_by(|a, b| {
            b.capped
                .sharpe
                .partial_cmp(&a.capped.sharpe)
                .unwrap()
                .then_with(|| {
                    b.capped
                        .aligned_return_pct
                        .partial_cmp(&a.capped.aligned_return_pct)
                        .unwrap()
                })
        });

        println!(
            "{:<20} {:>10} {:>8} {:>8} {:>10} || {:>10} {:>8} {:>8} {:>10}",
            "Strategy",
            "UncapRet",
            "UShp",
            "UMaxDD",
            "UAvgAct",
            "CapRet",
            "CShp",
            "CMaxDD",
            "CAvgAct"
        );
        println!("{}", "-".repeat(116));
        for row in &rows {
            println!(
                "{:<20} {:>9.1}% {:>8.2} {:>7.1}% {:>10.2} || {:>9.1}% {:>8.2} {:>7.1}% {:>10.2}",
                row.strategy.name(),
                row.uncapped.aligned_return_pct,
                row.uncapped.sharpe,
                row.uncapped.max_dd_pct,
                row.uncapped.avg_active_positions,
                row.capped.aligned_return_pct,
                row.capped.sharpe,
                row.capped.max_dd_pct,
                row.capped.avg_active_positions,
            );
        }

        println!("\nCapped-book detail:");
        for row in &rows {
            println!(
                "- {:<18} CapRet {:>8.1}% | Sharpe {:>5.2} | MaxDD {:>5.1}% | Trades {:>4} | Win {:>5.1}% | Idle {:>5.1}% | Gap vs trade-sum {:>+7.1}%",
                row.strategy.name(),
                row.capped.aligned_return_pct,
                row.capped.sharpe,
                row.capped.max_dd_pct,
                row.capped.trades,
                row.capped.win_rate_pct,
                row.capped.idle_days_pct,
                row.capped.aligned_return_pct - row.capped.trade_sum_return_pct,
            );
        }

        universe_summaries.push(UniverseSummary {
            label: label.to_string(),
            symbols: symbols.iter().map(|s| s.to_string()).collect(),
            rows,
        });
    }

    let snapshot_paths = write_snapshot(&universe_summaries)?;
    println!("\nPortfolio-stress snapshots written:");
    println!("- {}", SNAPSHOT_LATEST_MD);
    println!("- {}", SNAPSHOT_LATEST_CSV);
    println!("- {}", snapshot_paths.0);
    println!("- {}", snapshot_paths.1);

    println!("\nNotes:");
    println!("- Uncapped = equal-weight across all concurrent active trades.");
    println!(
        "- Capped = equal-weight across only the top-strength active trades each day (top-{}).",
        POSITION_CAP
    );
    println!("- Strength is signal-specific: MACD spread or breakout distance vs lookback range.");
    println!("- If a leader only looks good when the book is effectively unconstrained, that is a portfolio-realism warning.");

    Ok(())
}

struct UniverseData {
    data: Vec<(String, DataFrame)>,
    steps: usize,
}

fn aligned_universe(
    raw_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<UniverseData> {
    let min_len = symbols
        .iter()
        .filter_map(|symbol| raw_cache.get(*symbol).map(|df| df.height()))
        .min()
        .ok_or_else(|| anyhow::anyhow!("empty universe"))?;
    let steps = min_len.saturating_sub(1);
    if steps == 0 {
        anyhow::bail!("not enough data");
    }

    let mut data = Vec::new();
    for &symbol in symbols {
        let df = raw_cache
            .get(symbol)
            .ok_or_else(|| anyhow::anyhow!("missing symbol {symbol}"))?
            .slice(0, min_len);
        data.push((symbol.to_string(), df));
    }

    Ok(UniverseData { data, steps })
}

fn build_symbol_plans(universe: &UniverseData, strategy: StrategyKind) -> Result<Vec<SymbolPlan>> {
    universe
        .data
        .iter()
        .map(|(_, df)| build_symbol_plan(df, strategy))
        .collect()
}

fn build_symbol_plan(df: &DataFrame, strategy: StrategyKind) -> Result<SymbolPlan> {
    let signals = generate_signals(df, strategy)?;
    let strengths = generate_strengths(df, strategy)?;
    let open = df.column("open")?.f64()?;
    let n = open.len();
    let mut trades = Vec::new();
    let mut i = WARMUP_BARS;

    while i + HOLD_BARS + 1 < n {
        let signal = signals.get(i).copied().unwrap_or(0);
        if signal == 0 {
            i += 1;
            continue;
        }

        let entry_idx = i + 1;
        let exit_idx = i + 1 + HOLD_BARS;
        let entry = match open.get(entry_idx) {
            Some(v) if v > 0.0 => v,
            _ => {
                i += 1;
                continue;
            }
        };
        let exit = match open.get(exit_idx) {
            Some(v) if v > 0.0 => v,
            _ => {
                i += 1;
                continue;
            }
        };

        let gross_return = if signal > 0 {
            exit / entry - 1.0
        } else {
            entry / exit - 1.0
        };
        let net_return = gross_return - 2.0 * TAKER_FEE;
        trades.push(TradeWindow {
            entry_idx,
            exit_idx,
            strength: strengths.get(i).copied().unwrap_or(0.0).abs(),
            gross_return,
            net_return,
        });
        i = exit_idx;
    }

    Ok(SymbolPlan { trades })
}

fn simulate_portfolio(plans: &[SymbolPlan], steps: usize, cap: Option<usize>) -> PortfolioStats {
    let mut equity_curve = vec![1.0; steps + 1];
    let mut daily_returns = vec![0.0; steps];
    let mut active_counts = vec![0usize; steps + 1];
    let mut trade_sum_return_pct = 0.0;
    let mut trades = 0usize;
    let mut wins = 0usize;

    for plan in plans {
        for trade in &plan.trades {
            trade_sum_return_pct += trade.net_return * 100.0;
            trades += 1;
            if trade.net_return > 0.0 {
                wins += 1;
            }
        }
    }

    for day in 0..steps {
        let mut active = Vec::<(f64, f64)>::new();
        for plan in plans {
            for trade in &plan.trades {
                if day == trade.entry_idx {
                    active.push((trade.strength, -TAKER_FEE));
                }
                if day >= trade.entry_idx && day < trade.exit_idx {
                    let span = (trade.exit_idx - trade.entry_idx) as f64;
                    if span > 0.0 {
                        active.push((trade.strength, trade.gross_return / span));
                    }
                }
                if day == trade.exit_idx {
                    active.push((trade.strength, -TAKER_FEE));
                }
            }
        }

        active_counts[day] = active.len();
        if active.is_empty() {
            daily_returns[day] = 0.0;
            equity_curve[day + 1] = equity_curve[day];
            continue;
        }

        active.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        let selected_len = cap.unwrap_or(active.len()).min(active.len());
        let selected = &active[..selected_len];
        let avg_ret = selected.iter().map(|(_, r)| *r).sum::<f64>() / selected.len() as f64;
        daily_returns[day] = avg_ret;
        equity_curve[day + 1] = equity_curve[day] * (1.0 + avg_ret);
    }
    active_counts[steps] = active_counts[steps.saturating_sub(1)];

    let aligned_return_pct = (equity_curve.last().copied().unwrap_or(1.0) - 1.0) * 100.0;
    let sharpe = calc_sharpe_from_returns(&daily_returns);
    let max_dd_pct = calc_max_drawdown_pct(&equity_curve);
    let avg_active_positions =
        active_counts.iter().sum::<usize>() as f64 / active_counts.len() as f64;
    let idle_days_pct = active_counts.iter().filter(|&&c| c == 0).count() as f64
        / active_counts.len() as f64
        * 100.0;
    let win_rate_pct = if trades == 0 {
        0.0
    } else {
        wins as f64 / trades as f64 * 100.0
    };

    PortfolioStats {
        aligned_return_pct,
        sharpe,
        max_dd_pct,
        avg_active_positions,
        idle_days_pct,
        trade_sum_return_pct,
        trades,
        win_rate_pct,
    }
}

fn generate_signals(df: &DataFrame, strategy: StrategyKind) -> Result<Vec<i32>> {
    match strategy {
        StrategyKind::Macd => generate_macd_signals(df),
        StrategyKind::MacdRegime => generate_macd_regime_signals(df),
        StrategyKind::TurtleRegime => generate_turtle_regime_signals(df, PERIOD),
        StrategyKind::TurtleRegimeMacd => generate_turtle_regime_macd_signals(df, PERIOD),
    }
}

fn generate_strengths(df: &DataFrame, strategy: StrategyKind) -> Result<Vec<f64>> {
    match strategy {
        StrategyKind::Macd | StrategyKind::MacdRegime => generate_macd_strengths(df),
        StrategyKind::TurtleRegime | StrategyKind::TurtleRegimeMacd => {
            generate_turtle_strengths(df, PERIOD)
        }
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

fn generate_macd_strengths(df: &DataFrame) -> Result<Vec<f64>> {
    let macd = df.column("macd")?.f64()?;
    let signal = df.column("macd_signal")?.f64()?;
    let mut out = vec![0.0; macd.len()];
    for i in 0..macd.len() {
        out[i] = (macd.get(i).unwrap_or(0.0) - signal.get(i).unwrap_or(0.0)).abs();
    }
    Ok(out)
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

fn generate_turtle_strengths(df: &DataFrame, period: usize) -> Result<Vec<f64>> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let n = close.len();
    let mut out = vec![0.0; n];

    for i in period..n {
        let period_high = (i - period..i)
            .filter_map(|j| high.get(j))
            .fold(f64::NEG_INFINITY, f64::max);
        let period_low = (i - period..i)
            .filter_map(|j| low.get(j))
            .fold(f64::INFINITY, f64::min);
        let current_close = close.get(i).unwrap_or(0.0);
        let range = (period_high - period_low).abs().max(1e-9);
        if current_close > period_high {
            out[i] = (current_close - period_high) / range;
        } else if current_close < period_low {
            out[i] = (period_low - current_close) / range;
        }
    }

    Ok(out)
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

fn calc_sharpe_from_returns(returns: &[f64]) -> f64 {
    if returns.is_empty() {
        return 0.0;
    }
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

fn write_snapshot(universes: &[UniverseSummary]) -> Result<(String, String)> {
    fs::create_dir_all(SNAPSHOT_DIR)?;
    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let archive_md = format!("{}/trend_portfolio_stress_{}.md", SNAPSHOT_DIR, timestamp);
    let archive_csv = format!("{}/trend_portfolio_stress_{}.csv", SNAPSHOT_DIR, timestamp);

    let mut markdown = String::new();
    markdown.push_str("# Trend Portfolio Stress Audit Snapshot\n\n");
    markdown.push_str(&format!("- Timestamp (UTC): {}\n", Utc::now().to_rfc3339()));
    markdown.push_str(&format!("- Candles: {}\n", CANDLES));
    markdown.push_str(&format!("- Hold bars: {}\n", HOLD_BARS));
    markdown.push_str(&format!("- Fee each side: {:.3}%\n", TAKER_FEE * 100.0));
    markdown.push_str(&format!("- Warmup bars: {}\n", WARMUP_BARS));
    markdown.push_str(&format!(
        "- Position cap: top {} active trades by strength\n",
        POSITION_CAP
    ));
    markdown.push_str(&format!("- Stress universes: {}\n\n", universes.len()));

    markdown.push_str("## Capped-book winners by universe\n\n");
    markdown.push_str("| Universe | Symbols | Winner | Cap Return % | Sharpe | MaxDD % | Runner-up | Cap Return % | Sharpe |\n");
    markdown.push_str("|----------|---------|--------|-------------:|-------:|--------:|-----------|-------------:|-------:|\n");
    for universe in universes {
        let winner = &universe.rows[0];
        let runner_up = universe.rows.get(1).unwrap_or(winner);
        markdown.push_str(&format!(
            "| {} | {} | {} | {:.1} | {:.2} | {:.1} | {} | {:.1} | {:.2} |\n",
            universe.label,
            universe.symbols.join(", "),
            winner.strategy.name(),
            winner.capped.aligned_return_pct,
            winner.capped.sharpe,
            winner.capped.max_dd_pct,
            runner_up.strategy.name(),
            runner_up.capped.aligned_return_pct,
            runner_up.capped.sharpe,
        ));
    }

    markdown.push_str("\n## Full per-universe tables\n");
    for universe in universes {
        markdown.push_str(&format!("\n### {}\n\n", universe.label));
        markdown.push_str(&format!("Symbols: {}\n\n", universe.symbols.join(", ")));
        markdown.push_str("| Rank | Strategy | Uncap Ret % | Uncap Sharpe | Uncap MaxDD % | Cap Ret % | Cap Sharpe | Cap MaxDD % | Trades | Win % | Idle % | Cap Gap vs Trade Sum % |\n");
        markdown.push_str("|------|----------|------------:|-------------:|---------------:|----------:|-----------:|------------:|-------:|------:|-------:|-----------------------:|\n");
        for (idx, row) in universe.rows.iter().enumerate() {
            markdown.push_str(&format!(
                "| {} | {} | {:.1} | {:.2} | {:.1} | {:.1} | {:.2} | {:.1} | {} | {:.1} | {:.1} | {:+.1} |\n",
                idx + 1,
                row.strategy.name(),
                row.uncapped.aligned_return_pct,
                row.uncapped.sharpe,
                row.uncapped.max_dd_pct,
                row.capped.aligned_return_pct,
                row.capped.sharpe,
                row.capped.max_dd_pct,
                row.capped.trades,
                row.capped.win_rate_pct,
                row.capped.idle_days_pct,
                row.capped.aligned_return_pct - row.capped.trade_sum_return_pct,
            ));
        }
    }

    let mut csv = String::from(
        "universe,symbols,rank,strategy,uncapped_return_pct,uncapped_sharpe,uncapped_max_dd_pct,uncapped_avg_active_positions,uncapped_idle_days_pct,uncapped_trade_sum_return_pct,uncapped_trades,uncapped_win_rate_pct,capped_return_pct,capped_sharpe,capped_max_dd_pct,capped_avg_active_positions,capped_idle_days_pct,capped_trade_sum_return_pct,capped_trades,capped_win_rate_pct,capped_gap_vs_trade_sum_pct\n",
    );
    for universe in universes {
        for (idx, row) in universe.rows.iter().enumerate() {
            csv.push_str(&format!(
                "{},{},{},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{},{:.4},{:.4}\n",
                universe.label,
                universe.symbols.join("|"),
                idx + 1,
                row.strategy.name(),
                row.uncapped.aligned_return_pct,
                row.uncapped.sharpe,
                row.uncapped.max_dd_pct,
                row.uncapped.avg_active_positions,
                row.uncapped.idle_days_pct,
                row.uncapped.trade_sum_return_pct,
                row.uncapped.trades,
                row.uncapped.win_rate_pct,
                row.capped.aligned_return_pct,
                row.capped.sharpe,
                row.capped.max_dd_pct,
                row.capped.avg_active_positions,
                row.capped.idle_days_pct,
                row.capped.trade_sum_return_pct,
                row.capped.trades,
                row.capped.win_rate_pct,
                row.capped.aligned_return_pct - row.capped.trade_sum_return_pct,
            ));
        }
    }

    fs::write(SNAPSHOT_LATEST_MD, &markdown)?;
    fs::write(SNAPSHOT_LATEST_CSV, &csv)?;
    fs::write(&archive_md, markdown)?;
    fs::write(&archive_csv, csv)?;

    Ok((archive_md, archive_csv))
}
