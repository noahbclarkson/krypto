//! T86: REGIME_LOOKBACK optimization on exact-live Turtle-only path.
//!
//! Mission: Sweep REGIME_LOOKBACK (LB) on current exact-live semantics.
//! Current config LB=41 was tuned on EP=24 + CHAND dual-exit, but live path uses EP=21 + Turtle-only.
//! This is a re-validation on the correct path.
//!
//! Sweep: LB ∈ [5..=200] step 1 (196 values) × Base5 (6 symbols)
//! Baseline: current LB=41
//! Target: find robustness winner (pass rate + Sharpe) on exact-live path.

use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::Write as _;
use std::path::PathBuf;

use anyhow::Result;
use krypto::data::loader::{load_universe,Universe};
use krypto::features::regime::{RegimeDetector,RegimeConfig};
use krypto::paper::Backtest;
use krypto::turtle::{TurtleConfig,TurtleStrategy};

// Output base
fn output_dir() -> PathBuf { PathBuf::from("snapshots") }

// Parse comma-separated list
fn parse_symbols(s: &str) -> Vec<String> {
    s.split(',').map(|s| s.trim().to_string()).collect()
}

// Main sweep
fn main() -> Result<()> {
    // Base5 universe - aligned daily bars
    let symbols = vec![
        "BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"
    ];
    let warmup = 300;
    
    // Load full history
    let mut all_data: HashMap<String, Vec<krypto::paper::Bar>> = HashMap::new();
    for sym in &symbols {
        let path = format!("data/cache/{}_daily.csv", sym);
        match krypto::data::loader::load_csv(&path, warmup) {
            Ok(bars) => {
                println!("Loaded {} bars for {}", bars.len(), sym);
                all_data.insert(sym.clone(), bars);
            },
            Err(e) => {
                eprintln!("Skip {}: {}", sym, e);
            }
        }
    }
    if all_data.is_empty() {
        anyhow::bail!("No data loaded");
    }
    println!("Working with {} symbols", all_data.len());
    
    // Current baseline params
    let base_ep = 21;
    let base_atr = 24;
    let base_mult = 2.0;
    let base_hm = 15;
    let base_cap = 3;
    let base_ap = 17; // REGIME_ATR_PERIOD - keep at current
    let base_lb = 41; // current REGIME_LOOKBACK baseline
    let base_t = 5.0; // ATR_RANK_THRESHOLD
    
    // Hedge params (frozen)
    let hedge_ap = 38;
    let hedge_lb = 252;
    let hedge_pct = 0.45;
    let hedge_size = 0.25;
    let fee = 0.0004;
    
    // Sweep LB: 5 to 200, step 1
    let lb_values: Vec<usize> = (5..=200).collect();
    let mut results: Vec<(usize, f64, f64, f64, f64, usize)> = Vec::new();
    
    // CSV output
    let mut csv = File::create(output_dir().join("t86_regime_lookback_sweep.csv"))?;
    writeln!(csv, "LB,equity,sharpe,maxdd,return_pct,trades")?;
    
    println!("\nSweeping REGIME_LOOKBACK: {} values...", lb_values.len());
    
    for lb in &lb_values {
        let mut total_equity = 1.0;
        let mut wins = 0;
        let mut loss = 0.0;
        let mut max_dd = 0.0;
        let mut trade_count = 0;
        
        for (sym, bars) in &all_data {
            if bars.len() < 100 { continue; }
            
            // Build regime detector
            let regime = RegimeDetector::new(RegimeConfig {
                atr_period: base_ap,
                lookback: *lb,
                threshold: base_t,
            });
            
            // Build Turtle config
            let config = TurtleConfig {
                ep: base_ep,
                atr_period: base_atr,
                atr_mult: base_mult,
                hold_max: base_hm,
                position_cap: base_cap,
                regime: Some(regime),
                hedge_ap,
                hedge_lb,
                hedge_pct,
                hedge_size,
                fee,
                ..Default::default()
            };
            
            let strat = TurtleStrategy::new(config);
            let mut bt = Backtest::new(10000.0);
            
            // Run
            let _ = bt.run(strat, bars.clone());
            
            // Metrics
            total_equity *= bt.equity();
            max_dd = max_dd.max(bt.max_drawdown());
            trade_count += bt.trades().len();
            
            if bt.equity() > 1.0 { wins += 1; }
            else { loss += 1; }
        }
        
        // Daily account Sharpe approximation
        let daily_ret = (total_equity.powf(365.0 / 1797.0) - 1.0) * 100.0;
        let sharpe = if daily_ret.abs() > 0.01 { daily_ret / max_dd } else { 0.0 };
        
        results.push((*lb, total_equity, sharpe, max_dd, daily_ret * 100.0, trade_count));
        writeln!(csv, "{},{:.4},{:.3},{:.1},{:.1},{}", 
            lb, total_equity, sharpe, max_dd * 100.0, daily_ret, trade_count)?;
    }
    
    // Find winner
    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    
    println!("\n=== TOP 10 LB VALUES (by equity) ===");
    for (lb, eq, sh, dd, ret, tc) in results.iter().take(10) {
        println!("LB={}: equity={:.4}, sharpe={:.3}, DD={:.1}%, ret={:.1}%, trades={}", 
            lb, eq, sh, dd * 100.0, ret, tc);
    }
    
    // Find best by Sharpe (robustness)
    results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
    
    println!("\n=== TOP 10 LB VALUES (by Sharpe) ===");
    for (lb, eq, sh, dd, ret, tc) in results.iter().take(10) {
        println!("LB={}: equity={:.4}, sharpe={:.3}, DD={:.1}%, ret={:.1}%, trades={}", 
            lb, eq, sh, dd * 100.0, ret, tc);
    }
    
    // Winner by pass rate
    let baseline_equity = results.iter().find(|x| x.0 == base_lb).map(|x| x.1).unwrap_or(1.0);
    
    // Compare baseline vs best
    let best = &results[0];
    println!("\n=== COMPARISON ===");
    println!("Baseline LB={}: equity={:.4}", base_lb, baseline_equity);
    println!("Winner  LB={}: equity={:.4}", best.0, best.1);
    println!("Delta        : {:+.2}%", (best.1 / baseline_equity - 1.0) * 100.0);
    
    // Write summary
    let mut md = File::create(output_dir().join("t86_regime_lookback.md"))?;
    writeln!(md, "# T86: REGIME_LOOKBACK Sweep on Exact-Live Path")?;
    writeln!(md, "")?;
    writeln!(md, "## Config")?;
    writeln!(md, "- EP=21, TurtleATR(24, 2.0), HM=15, CAP=3, fee=4bp")?;
    writeln!(md, "- REGIME_ATR_PERIOD=17, ATR_RANK_THRESHOLD=5.0")?;
    writeln!(md, "- HEDGE(AP=38, LB=252, PCT=0.45, SIZE=0.25)")?;
    writeln!(md, "")?;
    writeln!(md, "## Sweep")?;
    writeln!(md, "- LB ∈ [5..=200] step 1 (196 values)")?;
    writeln!(md, "- Universe: Base5 (BTC/ETH/SOL/XRP/DOGE/ADA)")?;
    writeln!(md, "")?;
    writeln!(md, "## Results")?;
    writeln!(md, "| LB | Equity | Sharpe | MaxDD | Return% | Trades |")?;
    writeln!(md, "|----|--------|-------|-------|--------|--------|")?;
    for (lb, eq, sh, dd, ret, tc) in results.iter().take(20) {
        writeln!(md, "| {} | {:.4} | {:.3} | {:.1}% | {:.1}% | {} |", 
            lb, eq, sh, dd*100.0, ret, tc)?;
    }
    
    // Check if current LB=41 is still optimal
    let baseline_pos = results.iter().position(|x| x.0 == base_lb);
    if let Some(pos) = baseline_pos {
        let rank = pos + 1;
        println!("\nCurrent LB={} ranks #{} by Sharpe", base_lb, rank);
    }
    
    Ok(())
}