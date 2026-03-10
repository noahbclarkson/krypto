//! Full strategy backtest — tests every strategy across all symbols/intervals.
//!
//! Uses the StrategyRegistry so new strategies are automatically included.
//! Data is cached to parquet so re-runs are fast (no re-fetching).
//!
//! Usage:
//!   cargo run --release --example full_strategy_backtest
//!   cargo run --release --example full_strategy_backtest -- --candles 3000

use anyhow::Result;
use colored::*;
use krypto::{
    algo::StrategyRegistry,
    backtest::engine::{Backtester, PositionSizing},
    data::{loader::DataLoader, CacheConfig},
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::collections::HashMap;
use std::time::Instant;

// ──────────────────────── tunables ────────────────────────────────────────────
const CANDLES: u32 = 3000;
const CAPITAL: f64 = 10_000.0;
const TRAILING_STOP: f64 = 0.05; // 5 %
const TAKE_PROFIT: f64 = 0.15; // 15 % (0.0 to disable)

const SYMBOLS: &[&str] = &["BTCUSDT", "ETHUSDT", "SOLUSDT", "BNBUSDT"];
const INTERVALS: &[&str] = &["1h", "4h", "1d"];

// Min trades to be considered statistically meaningful
const MIN_TRADES: usize = 15;
// ──────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct RunResult {
    strategy: String,
    symbol: String,
    interval: String,
    candles: usize,
    total_trades: usize,
    win_rate: f64,
    profit_factor: f64,
    total_return_pct: f64,
    max_drawdown_pct: f64,
    sharpe: f64,
    sortino: f64,
    calmar: f64,
    avg_duration_bars: f64,
}

impl RunResult {
    /// Composite score used for ranking. Balances return, risk, and reliability.
    fn score(&self) -> f64 {
        if self.total_trades < MIN_TRADES {
            return f64::NEG_INFINITY;
        }
        // Calmar (return / max-DD) penalised by profit factor and win rate.
        // Negative profit_factor or return → negative score.
        let base = if self.max_drawdown_pct > 0.0 {
            self.total_return_pct / self.max_drawdown_pct
        } else {
            self.total_return_pct
        };
        base * self.profit_factor.max(0.0) * (self.win_rate / 100.0)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let candles: u16 = args.windows(2)
        .find(|w| w[0] == "--candles")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(CANDLES);

    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "  KRYPTO — Full Strategy Backtest".bright_cyan().bold());
    println!("{}", "━".repeat(72).bright_cyan());
    println!(
        "  candles={} | capital=${:.0} | stop={:.0}% | tp={:.0}%\n",
        candles, CAPITAL, TRAILING_STOP * 100.0, TAKE_PROFIT * 100.0
    );

    // Use persistent cache
    let cache = CacheConfig { enabled: true, cache_dir: "data/cache".into() };
    let loader = DataLoader::with_cache(None, None, cache);
    let registry = StrategyRegistry::new();
    let strategy_names: Vec<String> = {
        let mut names = registry.available_strategies()
            .into_iter().map(|s| s.to_string()).collect::<Vec<_>>();
        names.sort();
        names
    };

    println!("  Strategies : {}", strategy_names.len());
    println!("  Symbols    : {}", SYMBOLS.join(", "));
    println!("  Intervals  : {}", INTERVALS.join(", "));
    println!(
        "  Combos     : {}\n",
        strategy_names.len() * SYMBOLS.len() * INTERVALS.len()
    );

    // ── Phase 1: fetch + cache all data ──────────────────────────────────────
    println!("{}", "Phase 1 — Fetching data (cached after first run)".bright_green());
    let mut data_cache: HashMap<String, DataFrame> = HashMap::new();

    for sym in SYMBOLS {
        for int in INTERVALS {
            let key = format!("{sym}_{int}");
            print!("  {:<18} ", key);
            let t = Instant::now();

            match loader.fetch_data(sym, int, candles).await {
                Ok(raw) => {
                    let df = FeatureEngine::add_technicals(&raw, None)?;
                    let n = df.height();
                    println!("{} {} bars  ({:.1}s)", "✓".green(), n, t.elapsed().as_secs_f64());
                    data_cache.insert(key, df);
                }
                Err(e) => println!("{} {e}", "✗".red()),
            }
        }
    }

    // ── Phase 2: backtest every strategy × every dataset ─────────────────────
    println!("\n{}", "Phase 2 — Running backtests".bright_green());
    let backtester = Backtester::with_defaults(CAPITAL)
        .with_position_sizing(PositionSizing::Full);

    let total = strategy_names.len() * data_cache.len();
    let mut done = 0usize;
    let mut results: Vec<RunResult> = Vec::new();

    for name in &strategy_names {
        for (key, df) in &data_cache {
            done += 1;
            let parts: Vec<&str> = key.splitn(2, '_').collect();
            let (sym, int) = (parts[0], parts.get(1).copied().unwrap_or("?"));

            print!("\r  [{done:>3}/{total}] {name:<26} {sym:<9} {int}    ");
            let _ = std::io::Write::flush(&mut std::io::stdout());

            let mut strat = match registry.create(name) {
                Some(s) => s,
                None => continue,
            };

            let signals = match strat.predict(df) {
                Ok(s) => s,
                Err(_) => continue,
            };

            let res = match backtester.run(df, &signals, TRAILING_STOP, TAKE_PROFIT) {
                Ok(r) => r,
                Err(_) => continue,
            };

            if res.total_trades == 0 { continue; }

            results.push(RunResult {
                strategy: name.clone(),
                symbol: sym.to_string(),
                interval: int.to_string(),
                candles: df.height(),
                total_trades: res.total_trades,
                win_rate: res.win_rate,
                profit_factor: res.profit_factor,
                total_return_pct: res.total_return_pct,
                max_drawdown_pct: res.max_drawdown_pct,
                sharpe: res.sharpe_ratio,
                sortino: res.sortino_ratio,
                calmar: res.calmar_ratio,
                avg_duration_bars: res.avg_trade_duration_bars,
            });
        }
    }
    println!("\n");

    // ── Phase 3: rank and print ───────────────────────────────────────────────
    results.sort_by(|a, b| b.score().partial_cmp(&a.score()).unwrap_or(std::cmp::Ordering::Equal));

    println!("{}", "━".repeat(100).bright_cyan());
    println!("{}", "  ALL RESULTS — sorted by composite score (return/DD × PF × win-rate)".bright_cyan().bold());
    println!("{}", "━".repeat(100).bright_cyan());
    println!(
        "  {:<26} {:<10} {:<5} {:>7} {:>7} {:>6} {:>8} {:>7} {:>7} {:>5}",
        "Strategy", "Symbol", "Int", "Return%", "WinRate", "PF", "MaxDD%", "Sharpe", "Sortino", "Trades"
    );
    println!("{}", "─".repeat(100));

    let mut rank = 0usize;
    for r in &results {
        rank += 1;
        let ret_col = if r.total_return_pct > 0.0 {
            format!("{:>7.1}", r.total_return_pct).green().to_string()
        } else {
            format!("{:>7.1}", r.total_return_pct).red().to_string()
        };
        let mark = if r.total_trades < MIN_TRADES { "*".yellow() } else { " ".normal() };
        println!(
            "{mark} {rank:>2}. {:<24} {:<10} {:<5} {ret_col} {:>6.1}% {:>6.2} {:>7.1}% {:>7.2} {:>7.2} {:>5}",
            r.strategy, r.symbol, r.interval,
            r.win_rate, r.profit_factor, r.max_drawdown_pct,
            r.sharpe, r.sortino, r.total_trades
        );
    }
    println!("{} * fewer than {MIN_TRADES} trades — statistically unreliable", " ".normal());

    // ── Per-strategy averages ─────────────────────────────────────────────────
    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "  Strategy Average Returns (across all symbols/intervals)".bright_cyan().bold());
    println!("{}", "━".repeat(72).bright_cyan());

    let mut by_strat: HashMap<String, Vec<f64>> = HashMap::new();
    for r in &results {
        by_strat.entry(r.strategy.clone()).or_default().push(r.total_return_pct);
    }
    let mut strat_avgs: Vec<(String, f64, usize)> = by_strat.iter()
        .map(|(k, v)| (k.clone(), v.iter().sum::<f64>() / v.len() as f64, v.len()))
        .collect();
    strat_avgs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    for (i, (name, avg, count)) in strat_avgs.iter().enumerate() {
        let avg_str = if *avg > 0.0 {
            format!("{avg:>8.1}%").green().to_string()
        } else {
            format!("{avg:>8.1}%").red().to_string()
        };
        println!("  {:>2}. {:<26} avg return {avg_str}  ({count} runs)", i + 1, name);
    }

    // ── Per-symbol averages ───────────────────────────────────────────────────
    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "  Symbol Average Returns (across all strategies/intervals)".bright_cyan().bold());
    println!("{}", "━".repeat(72).bright_cyan());

    let mut by_sym: HashMap<String, Vec<f64>> = HashMap::new();
    for r in &results { by_sym.entry(r.symbol.clone()).or_default().push(r.total_return_pct); }
    let mut sym_avgs: Vec<(String, f64)> = by_sym.iter()
        .map(|(k, v)| (k.clone(), v.iter().sum::<f64>() / v.len() as f64)).collect();
    sym_avgs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    for (i, (sym, avg)) in sym_avgs.iter().enumerate() {
        let avg_str = if *avg > 0.0 { format!("{avg:>8.1}%").green().to_string() } else { format!("{avg:>8.1}%").red().to_string() };
        println!("  {:>2}. {:<12} avg return {avg_str}", i + 1, sym);
    }

    // ── Per-interval averages ─────────────────────────────────────────────────
    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "  Interval Average Returns (across all strategies/symbols)".bright_cyan().bold());
    println!("{}", "━".repeat(72).bright_cyan());

    let mut by_int: HashMap<String, Vec<f64>> = HashMap::new();
    for r in &results { by_int.entry(r.interval.clone()).or_default().push(r.total_return_pct); }
    let mut int_avgs: Vec<(String, f64)> = by_int.iter()
        .map(|(k, v)| (k.clone(), v.iter().sum::<f64>() / v.len() as f64)).collect();
    int_avgs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    for (i, (int, avg)) in int_avgs.iter().enumerate() {
        let avg_str = if *avg > 0.0 { format!("{avg:>8.1}%").green().to_string() } else { format!("{avg:>8.1}%").red().to_string() };
        println!("  {:>2}. {:<6} avg return {avg_str}", i + 1, int);
    }

    // ── Summary ───────────────────────────────────────────────────────────────
    let profitable = results.iter().filter(|r| r.total_return_pct > 0.0 && r.total_trades >= MIN_TRADES).count();
    let sig = results.iter().filter(|r| r.total_trades >= MIN_TRADES).count();
    let best = results.iter().find(|r| r.total_trades >= MIN_TRADES);
    let avg_ret = results.iter().map(|r| r.total_return_pct).sum::<f64>() / results.len().max(1) as f64;

    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "  Summary".bright_cyan().bold());
    println!("{}", "━".repeat(72).bright_cyan());
    println!("  Total results:         {}", results.len());
    println!("  Statistically valid:   {} (≥{MIN_TRADES} trades)", sig);
    println!("  Profitable (of valid): {}", profitable);
    println!("  Average return:        {:.1}%", avg_ret);
    if let Some(b) = best {
        println!("\n  {} {} {} {}: {:.1}% return, {:.1}% win-rate, {:.2} PF, {:.1}% DD",
            "Best:".bright_green().bold(),
            b.strategy.bright_cyan(), b.symbol.yellow(), b.interval,
            b.total_return_pct, b.win_rate, b.profit_factor, b.max_drawdown_pct
        );
    }
    println!();

    Ok(())
}
