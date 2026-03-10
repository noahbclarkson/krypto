//! Parameter sweep for top strategies with adaptive stops.
//!
//! Tests bollinger_reversion and volatility_squeeze across different
//! stop sizes, take profit levels, and timeframes.

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

const CANDLES: u32 = 3000;
const CAPITAL: f64 = 10_000.0;

// Focus on SOL since that's where we saw signal
const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "BNBUSDT", "XRPUSDT", 
    "ADAUSDT", "DOGEUSDT", "AVAXUSDT", "DOTUSDT", "LINKUSDT"
];
const INTERVALS: &[&str] = &["4h", "1d"];
const STRATEGIES: &[&str] = &["bollinger_reversion", "volatility_squeeze"];

// Parameter grid
const STOP_PCTS: &[f64] = &[0.05, 0.08, 0.10, 0.15, 0.20];
const TP_PCTS: &[f64] = &[0.0, 0.10, 0.15, 0.20, 0.30];

#[derive(Debug, Clone)]
struct SweepResult {
    strategy: String,
    symbol: String,
    interval: String,
    stop_pct: f64,
    tp_pct: f64,
    trades: usize,
    win_rate: f64,
    pf: f64,
    return_pct: f64,
    max_dd: f64,
}

impl SweepResult {
    fn score(&self) -> f64 {
        if self.trades < 15 || self.pf <= 0.0 {
            return f64::NEG_INFINITY;
        }
        let dd_factor = if self.max_dd > 0.0 { 1.0 / (1.0 + self.max_dd / 50.0) } else { 1.0 };
        self.return_pct * dd_factor * self.pf * (self.win_rate / 100.0)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(60).bright_cyan());
    println!("{}", "  STRATEGY PARAMETER SWEEP".bright_cyan().bold());
    println!("{}", "━".repeat(60).bright_cyan());
    println!("  Stops: {:?}", STOP_PCTS);
    println!("  TPs:   {:?}", TP_PCTS);
    println!("  Strategies: {:?}", STRATEGIES);
    println!("  Symbols:    {:?}", SYMBOLS);
    println!();

    let cache = CacheConfig { enabled: true, cache_dir: "data/cache".into() };
    let loader = DataLoader::with_cache(None, None, cache);
    let registry = StrategyRegistry::new();

    // Load data
    let mut data: HashMap<String, DataFrame> = HashMap::new();
    for sym in SYMBOLS {
        for int in INTERVALS {
            let key = format!("{sym}_{int}");
            if data.contains_key(&key) { continue; }
            print!("  Loading {}... ", key);
            match loader.fetch_data(sym, int, CANDLES).await {
                Ok(df) => {
                    let df = FeatureEngine::add_technicals(&df, None)?;
                    println!("{} bars", df.height());
                    data.insert(key, df);
                }
                Err(e) => println!("{} {}", "✗".red(), e),
            }
        }
    }
    println!();

    let mut results: Vec<SweepResult> = Vec::new();
    let total = STRATEGIES.len() * SYMBOLS.len() * INTERVALS.len() * STOP_PCTS.len() * TP_PCTS.len();
    let mut done = 0;

    for strat_name in STRATEGIES {
        for (key, df) in &data {
            let parts: Vec<&str> = key.splitn(2, '_').collect();
            let (sym, int) = (parts[0], parts.get(1).copied().unwrap_or("?"));

            for &stop in STOP_PCTS {
                for &tp in TP_PCTS {
                    done += 1;
                    print!("\r  [{}/{}] {} {} {} stop={:.0}% tp={:.0}%   ", 
                        done, total, strat_name, sym, int, stop*100.0, tp*100.0);

                    let mut strat = match registry.create(strat_name) {
                        Some(s) => s,
                        None => continue,
                    };

                    let signals = match strat.predict(df) {
                        Ok(s) => s,
                        Err(_) => continue,
                    };

                    let bt = Backtester::with_defaults(CAPITAL)
                        .with_position_sizing(PositionSizing::Full);
                    let res = match bt.run(df, &signals, stop, tp) {
                        Ok(r) => r,
                        Err(_) => continue,
                    };

                    if res.total_trades > 0 {
                        results.push(SweepResult {
                            strategy: strat_name.to_string(),
                            symbol: sym.to_string(),
                            interval: int.to_string(),
                            stop_pct: stop,
                            tp_pct: tp,
                            trades: res.total_trades,
                            win_rate: res.win_rate,
                            pf: res.profit_factor,
                            return_pct: res.total_return_pct,
                            max_dd: res.max_drawdown_pct,
                        });
                    }
                }
            }
        }
    }
    println!("\n");

    // Sort by score
    results.sort_by(|a, b| b.score().partial_cmp(&a.score()).unwrap());

    println!("{}", "━".repeat(90).bright_cyan());
    println!("{}", "  TOP 20 PARAMETER COMBINATIONS".bright_cyan().bold());
    println!("{}", "━".repeat(90).bright_cyan());
    println!(
        "  {:<22} {:<10} {:<4} {:>6} {:>5} {:>6} {:>7} {:>7} {:>7}",
        "Strategy", "Symbol", "Int", "Stop%", "TP%", "Trades", "Win%", "PF", "Return%"
    );
    println!("{}", "─".repeat(90));

    for (i, r) in results.iter().take(20).enumerate() {
        let ret = if r.return_pct > 0.0 { 
            format!("{:>7.1}", r.return_pct).green().to_string() 
        } else { 
            format!("{:>7.1}", r.return_pct).red().to_string() 
        };
        println!(
            "  {:>2}. {:<20} {:<10} {:<4} {:>5.0}% {:>4.0}% {:>6} {:>6.1}% {:>6.2} {}",
            i + 1, r.strategy, r.symbol, r.interval,
            r.stop_pct * 100.0, r.tp_pct * 100.0, r.trades,
            r.win_rate, r.pf, ret
        );
    }

    // Best per strategy
    println!("\n{}", "━".repeat(60).bright_cyan());
    println!("{}", "  BEST CONFIG PER STRATEGY".bright_cyan().bold());
    println!("{}", "━".repeat(60).bright_cyan());

    for strat in STRATEGIES {
        if let Some(best) = results.iter().find(|r| r.strategy == *strat) {
            println!("\n  {}:", strat.bright_cyan());
            println!("    Symbol:   {}", best.symbol.yellow());
            println!("    Interval: {}", best.interval);
            println!("    Stop:     {:.0}%", best.stop_pct * 100.0);
            println!("    TP:       {:.0}%", best.tp_pct * 100.0);
            println!("    Trades:   {}", best.trades);
            println!("    Win Rate: {:.1}%", best.win_rate);
            println!("    PF:       {:.2}", best.pf);
            println!("    Return:   {:.1}%", best.return_pct);
            println!("    Max DD:   {:.1}%", best.max_dd);
        }
    }

    // Analysis
    println!("\n{}", "━".repeat(60).bright_cyan());
    println!("{}", "  KEY FINDINGS".bright_cyan().bold());
    println!("{}", "━".repeat(60).bright_cyan());

    // Impact of stop size (use ordered float as key)
    let mut stop_impact: std::collections::BTreeMap<u64, (f64, usize)> = std::collections::BTreeMap::new();
    for r in &results {
        let key = (r.stop_pct * 100.0).round() as u64;
        let entry = stop_impact.entry(key).or_insert((0.0, 0));
        entry.0 += r.return_pct;
        entry.1 += 1;
    }
    println!("\n  Avg return by stop size:");
    for (stop_pct, (sum, count)) in &stop_impact {
        let avg = sum / *count as f64;
        let avg_str = if avg > 0.0 { format!("{avg:>7.1}%").green().to_string() } else { format!("{avg:>7.1}%").red().to_string() };
        println!("    {}% stop → {} avg return ({} runs)", stop_pct, avg_str, count);
    }

    // Impact of TP
    let mut tp_impact: std::collections::BTreeMap<u64, (f64, usize)> = std::collections::BTreeMap::new();
    for r in &results {
        let key = (r.tp_pct * 100.0).round() as u64;
        let entry = tp_impact.entry(key).or_insert((0.0, 0));
        entry.0 += r.return_pct;
        entry.1 += 1;
    }
    println!("\n  Avg return by TP:");
    for (tp_pct, (sum, count)) in &tp_impact {
        let avg = sum / *count as f64;
        let avg_str = if avg > 0.0 { format!("{avg:>7.1}%").green().to_string() } else { format!("{avg:>7.1}%").red().to_string() };
        println!("    {}% TP → {} avg return ({} runs)", tp_pct, avg_str, count);
    }

    println!();
    Ok(())
}
