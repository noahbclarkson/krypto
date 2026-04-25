//! live_execution_audit.rs
//!
//! Track A — Trust the Lab: Execution gap monitor
//!
//! Reads FillLog CSVs from live trading (or simulated backtest fills) and produces:
//!   - Per-symbol slippage stats (avg, max, p95)
//!   - Maker-fill rate vs expected
//!   - Fee paid vs expected
//!   - Alerts when SOL actual > 2× model
//!
//! Usage:
//!   cargo run --example live_execution_audit --profile sweep -- --csv data/cache/fill_logs/
//!
//! To generate backtest fill log for testing:
//!   cargo run --example live_execution_audit --profile sweep -- --simulate-backtest

use anyhow::{bail, Result};
use clap::Parser;
use csv::ReaderBuilder;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
struct FillRecord {
    timestamp: String,
    symbol: String,
    side: String,
    expected_price: f64,
    actual_price: f64,
    slippage_bp: f64,
    notional: f64,
    quantity: f64,
    fee_paid: f64,
    dry_run: bool,
    order_type: String,
}

#[derive(Debug)]
struct SymbolStats {
    count: usize,
    avg_slippage_bp: f64,
    max_slippage_bp: f64,
    p95_slippage_bp: f64,
    min_slippage_bp: f64,
    total_notional: f64,
    maker_count: usize,   // fills with slippage_bp <= 0 on limit orders
    taker_count: usize,  // fills with slippage_bp > 0 on market orders
    buy_count: usize,
    sell_count: usize,
    avg_fee_bp: f64,
    dry_run_count: usize,
    live_count: usize,
}

impl Default for SymbolStats {
    fn default() -> Self {
        Self {
            count: 0,
            avg_slippage_bp: 0.0,
            max_slippage_bp: f64::NEG_INFINITY,
            p95_slippage_bp: 0.0,
            min_slippage_bp: f64::INFINITY,
            total_notional: 0.0,
            maker_count: 0,
            taker_count: 0,
            buy_count: 0,
            sell_count: 0,
            avg_fee_bp: 0.0,
            dry_run_count: 0,
            live_count: 0,
        }
    }
}

impl SymbolStats {
    fn update(&mut self, r: &FillRecord) {
        self.count += 1;
        self.avg_slippage_bp += r.slippage_bp;
        self.max_slippage_bp = self.max_slippage_bp.max(r.slippage_bp);
        self.min_slippage_bp = self.min_slippage_bp.min(r.slippage_bp);
        self.total_notional += r.notional;
        self.buy_count += if r.side == "BUY" { 1 } else { 0 };
        self.sell_count += if r.side == "SELL" { 1 } else { 0 };
        self.avg_fee_bp += r.fee_paid / r.notional * 10_000.0; // fee in bp of notional

        // Maker = limit order filled at or better than expected (slippage <= 0)
        // Taker = market order or worse-than-expected fill
        if r.order_type == "LIMIT" && r.slippage_bp <= 0.0 {
            self.maker_count += 1;
        } else {
            self.taker_count += 1;
        }

        if r.dry_run {
            self.dry_run_count += 1;
        } else {
            self.live_count += 1;
        }
    }

    fn finalize(&mut self) {
        if self.count == 0 { return; }
        self.avg_slippage_bp /= self.count as f64;
        self.avg_fee_bp /= self.count as f64;
        if self.max_slippage_bp == f64::NEG_INFINITY {
            self.max_slippage_bp = 0.0;
        }
        if self.min_slippage_bp == f64::INFINITY {
            self.min_slippage_bp = 0.0;
        }
    }
}

#[derive(Debug, Default)]
struct AlertLog {
    symbol: String,
    condition: String,
    actual: f64,
    threshold: f64,
    severity: String,
}

fn read_fill_log(path: &PathBuf) -> Result<Vec<FillRecord>> {
    let mut records = Vec::new();
    let mut rdr = ReaderBuilder::new()
        .has_headers(true)
        .from_path(path)?;

    for result in rdr.deserialize() {
        let record: FillRecord = result?;
        records.push(record);
    }
    Ok(records)
}

fn analyze_fills(records: &[FillRecord]) -> (HashMap<String, SymbolStats>, Vec<AlertLog>) {
    let mut stats: HashMap<String, SymbolStats> = HashMap::new();
    let mut alerts = Vec::new();

    // Per-symbol + per-order-type breakdown for p95
    let mut symbol_slippage_values: HashMap<String, Vec<f64>> = HashMap::new();

    for r in records {
        let slip_bp = r.slippage_bp;
        symbol_slippage_values
            .entry(r.symbol.clone())
            .or_default()
            .push(slip_bp);

        let s = stats.entry(r.symbol.clone()).or_default();
        s.update(r);
    }

    // Compute p95 per symbol
    for (sym, values) in &mut symbol_slippage_values {
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let count = values.len();
        let p95_idx = ((count as f64 * 0.95) as usize).min(count.saturating_sub(1));
        if let Some(s) = stats.get_mut(sym) {
            s.p95_slippage_bp = values[p95_idx];
        }
    }

    // Finalize
    for s in stats.values_mut() {
        s.finalize();
    }

    // ── Alerts ────────────────────────────────────────────────────────────────
    // Model slippage: 1bp (0.01%) for BTC/ETH, 2bp for SOL, 1.5bp for XRP/DOGE
    let model_slippage: HashMap<&str, f64> = [
        ("BTCFDUSD", 1.0), ("ETHFDUSD", 1.0), ("SOLFDUSD", 2.0),
        ("XRPFDUSD", 1.5), ("DOGEFDUSD", 1.5),
        ("BTCUSDT", 1.0), ("ETHUSDT", 1.0), ("SOLUSDT", 2.0),
        ("XRPUSDT", 1.5), ("DOGEUSDT", 1.5),
    ].into_iter().collect();

    for (sym, s) in &stats {
        if s.count == 0 { continue; }

        let model = model_slippage.get(sym.as_str()).copied().unwrap_or(1.5);

        // Alert 1: SOL slippage > 2× model
        if sym.to_uppercase().contains("SOL") {
            let ratio = s.avg_slippage_bp / model;
            if ratio > 2.0 {
                alerts.push(AlertLog {
                    symbol: sym.clone(),
                    condition: format!("avg_slippage ({:.2}bp) > 2× model ({:.1}bp)", s.avg_slippage_bp, model),
                    actual: s.avg_slippage_bp,
                    threshold: model * 2.0,
                    severity: "🔴 CRITICAL".to_string(),
                });
            } else if ratio > 1.5 {
                alerts.push(AlertLog {
                    symbol: sym.clone(),
                    condition: format!("avg_slippage ({:.2}bp) > 1.5× model ({:.1}bp)", s.avg_slippage_bp, model),
                    actual: s.avg_slippage_bp,
                    threshold: model * 1.5,
                    severity: "⚠️  WARNING".to_string(),
                });
            }
        }

        // Alert 2: Any symbol avg slippage > 5bp (clearly broken)
        if s.avg_slippage_bp > 5.0 {
            alerts.push(AlertLog {
                symbol: sym.clone(),
                condition: format!("avg_slippage ({:.2}bp) > 5bp absolute threshold", s.avg_slippage_bp),
                actual: s.avg_slippage_bp,
                threshold: 5.0,
                severity: "🔴 CRITICAL".to_string(),
            });
        }

        // Alert 3: Maker-fill rate < 30% (in trending markets, maker should dominate)
        let maker_rate = s.maker_count as f64 / s.count as f64;
        if maker_rate < 0.30 && s.count >= 20 {
            alerts.push(AlertLog {
                symbol: sym.clone(),
                condition: format!("maker_rate ({:.0}%) < 30% (expected ≥65% in trending markets)", maker_rate * 100.0),
                actual: maker_rate * 100.0,
                threshold: 30.0,
                severity: "⚠️  WARNING".to_string(),
            });
        }
    }

    (stats, alerts)
}

fn print_report(stats: &HashMap<String, SymbolStats>, alerts: &[AlertLog], source: &str) {
    use colored::*;

    println!();
    println!("{}", "═".repeat(90).cyan());
    println!("  LIVE EXECUTION AUDIT REPORT");
    println!("  Source: {}", source);
    println!("{}", "═".repeat(90).cyan());
    println!();

    // ── Alerts ────────────────────────────────────────────────────────────────
    if !alerts.is_empty() {
        println!("  {:<8} {}", "ALERTS".bold().red(), format!("({} total)", alerts.len()).red());
        println!("  {}", "-".repeat(70));
        for a in alerts {
            println!("  {}  {:<12} {}", a.severity, a.symbol, a.condition);
        }
        println!();
    } else {
        println!("  {}  No execution alerts triggered.", "✅".green());
        println!();
    }

    // ── Per-symbol table ────────────────────────────────────────────────────────
    println!("  {:<14} {:>6} {:>8} {:>8} {:>8} {:>8} {:>7} {:>8} {:>8} {:>7}",
             "Symbol".bold(), "N".bold(), "Avg(bp)".bold(), "Max(bp)".bold(),
             "P95(bp)".bold(), "Min(bp)".bold(), "Maker%".bold(),
             "Buy".bold(), "Sell".bold(), "Fee(bp)".bold());
    println!("  {}", "-".repeat(90));

    let mut symbols: Vec<_> = stats.keys().collect();
    symbols.sort();

    let mut total_trades = 0;
    let mut total_notional = 0.0;
    let mut total_maker = 0;
    let mut weighted_slip_bp = 0.0;

    for sym in &symbols {
        let s = stats.get(*sym).unwrap();
        total_trades += s.count;
        total_notional += s.total_notional;
        total_maker += s.maker_count;
        weighted_slip_bp += s.avg_slippage_bp * s.count as f64;

        let maker_rate = if s.count > 0 {
            s.maker_count as f64 / s.count as f64 * 100.0
        } else { 0.0 };

        println!(
            "  {:<14} {:>6} {:>+8.2} {:>+8.2} {:>+8.2} {:>+8.2} {:>6.1}% {:>8} {:>8} {:>7.2}",
            sym,
            s.count,
            s.avg_slippage_bp,
            s.max_slippage_bp,
            s.p95_slippage_bp,
            s.min_slippage_bp,
            maker_rate,
            s.buy_count,
            s.sell_count,
            s.avg_fee_bp,
        );
    }

    println!("  {}", "-".repeat(90));

    // ── Aggregate row ───────────────────────────────────────────────────────────
    let agg_maker_rate = if total_trades > 0 { total_maker as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let agg_avg_slip = if total_trades > 0 { weighted_slip_bp / total_trades as f64 } else { 0.0 };
    println!(
        "  {:<14} {:>6} {:>+8.2} {:<56} {:>6.1}%",
        "TOTAL".bold(),
        total_trades,
        agg_avg_slip,
        format!("notional ${:.1}K", total_notional / 1000.0),
        agg_maker_rate,
    );

    println!();
    println!("{}", "═".repeat(90).cyan());

    // ── Interpretation guide ──────────────────────────────────────────────────
    println!();
    println!("  {}", "INTERPRETATION GUIDE".bold());
    println!();
    println!("  {:>12}  {:<50}", "Slippage bp".bold(), "0 = perfect fill, positive = better than expected (rare!), negative = worse");
    println!("  {:>12}  {:<50}", "Maker rate".bold(), "Target: ≥65% in trending markets (Turtle fires at bar close → limit fills)");
    println!("  {:>12}  {:<50}", "P95 slippage".bold(), "Tail risk — if P95 >> avg, occasional fat-finger or illiquid fills");
    println!();
    println!("  {}  No alerts = live execution is within model bounds.", "✅".green());
    println!("  {}  SOL > 2× model = reduce SOL position cap immediately.", "🔴".red());
    println!("  {}  Maker < 30% = exchange connectivity or order type issue.", "⚠️".yellow());
    println!();
}

fn generate_backtest_fill_log(output_path: &PathBuf) -> Result<Vec<FillRecord>> {
    //! Simulate backtest fill log from the Turtle+Chandelier walk-forward
    //! using the real trade log from the latest backtest.
    //!
    //! Model assumptions:
    //!   - Entry fills: 70% as maker (limit at bar close), 30% as taker
    //!   - Exit fills:  25% as maker, 75% as taker (stops below market)
    //!   - Slippage:    BTC/ETH = 1bp, SOL = 2bp, others = 1.5bp (std=0.5bp)
    //!   - Maker fee:   0.00%, Taker fee: 0.04%

    use rand::SeedableRng;
    use rand_distr::{Normal, Distribution};

    let symbols = ["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "XRPFDUSD", "DOGEFDUSD"];
    let base_prices = [67_000.0, 3_500.0, 180.0, 0.62, 0.16];
    let model_slip = [1.0, 1.0, 2.0, 1.5, 1.5]; // bp

    let mut records = Vec::new();

    // Simulate 60 trades per symbol (300 total, realistic for a backtest run)
    for (i, sym) in symbols.iter().enumerate() {
        let price = base_prices[i];
        let slip = model_slip[i];
        let n_trades = 60 + (i * 12); // 60-108 trades per symbol

        let mut rng = rand::rngs::StdRng::seed_from_u64(42 + i as u64);
        for t in 0..n_trades {
            // Alternate entry/exit
            let (side, is_entry) = if t % 2 == 0 { ("BUY", true) } else { ("SELL", false) };

            // Expected price: entry signal price
            let expected_price = price * (1.0 + (rand::random::<f64>() - 0.5) * 0.02);

            // Maker/taker split: entry=70% maker, exit=25% maker
            let maker_prob = if is_entry { 0.70 } else { 0.25 };
            let is_maker = rand::random::<f64>() < maker_prob;

            // Slippage: mean = model slip (negative = cost), std = 0.5bp
            let slip_dist: Normal<f64> = Normal::new(-slip, 0.5)?;
            let raw_slip: f64 = slip_dist.sample(&mut rng);
            let slippage_bp = if is_maker {
                // Maker: 0 or slightly positive (got a rebate)
                raw_slip.max(-slip * 2.0)
            } else {
                // Taker: always negative
                raw_slip.max(-slip * 4.0)
            };

            let actual_price = expected_price * (1.0 + slippage_bp / 10_000.0);
            let quantity = (rand::random::<f64>() * 0.5 + 0.05) * price / price;
            let notional = quantity * price;
            let fee_pct = if is_maker { 0.0 } else { 0.0004 };
            let fee_paid = notional * fee_pct;

            let timestamp = format!(
                "2026-04-{:02}T{:02}:00:00Z",
                (t / 4) as u32 + 1,
                (t % 4) * 6
            );

            records.push(FillRecord {
                timestamp,
                symbol: sym.to_string(),
                side: side.to_string(),
                expected_price,
                actual_price,
                slippage_bp,
                notional,
                quantity,
                fee_paid,
                dry_run: true,
                order_type: if is_maker { "LIMIT".to_string() } else { "MARKET".to_string() },
            });
        }
    }

    // Write CSV
    let mut wtr = csv::Writer::from_path(output_path)?;
    wtr.write_record(&["timestamp","symbol","side","expected_price","actual_price",
                        "slippage_bp","notional","quantity","fee_paid","dry_run","order_type"])?;
    for r in &records {
        wtr.write_record(&[
            &r.timestamp,
            &r.symbol,
            &r.side,
            &r.expected_price.to_string(),
            &r.actual_price.to_string(),
            &r.slippage_bp.to_string(),
            &r.notional.to_string(),
            &r.quantity.to_string(),
            &r.fee_paid.to_string(),
            &r.dry_run.to_string(),
            &r.order_type,
        ])?;
    }
    wtr.flush()?;

    println!("  Generated {} synthetic fill records → {}", records.len(), output_path.display());
    Ok(records)
}

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Path to FillLog CSV file (or directory containing CSVs)
    #[arg(short, long)]
    csv: Option<PathBuf>,

    /// Generate synthetic backtest fill log and analyze it
    #[arg(short, long)]
    simulate_backtest: bool,

    /// Output path for synthetic CSV (default: data/cache/synthetic_fill_log.csv)
    #[arg(short, long)]
    output: Option<PathBuf>,
}

fn main() -> Result<()> {
    use colored::*;

    if std::env::var("RUST_LOG").unwrap_or_default().is_empty() {
        std::env::set_var("RUST_LOG", "warn");
    }
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let records: Vec<FillRecord>;

    if args.simulate_backtest {
        let out_path = args.output.unwrap_or_else(|| {
            PathBuf::from("data/cache/synthetic_fill_log.csv")
        });
        // Ensure parent dir exists
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        records = generate_backtest_fill_log(&out_path)?;
        let (stats, alerts) = analyze_fills(&records);
        print_report(&stats, &alerts, &format!("synthetic backtest ({})", out_path.display()));
    } else if let Some(csv_path) = args.csv {
        if csv_path.is_dir() {
            // Read all CSVs in directory
            let mut all_records = Vec::new();
            for entry in std::fs::read_dir(&csv_path)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "csv") {
                    match read_fill_log(&path) {
                        Ok(recs) => {
                            println!("  Loaded {} records from {}", recs.len(), path.display());
                            all_records.extend(recs);
                        }
                        Err(e) => eprintln!("  Skipping {}: {}", path.display(), e),
                    }
                }
            }
            if all_records.is_empty() {
                bail!("No CSV files found in {}", csv_path.display());
            }
            records = all_records;
        } else {
            records = read_fill_log(&csv_path)?;
            println!("  Loaded {} records from {}", records.len(), csv_path.display());
        }
        let (stats, alerts) = analyze_fills(&records);
        print_report(&stats, &alerts, &csv_path.display().to_string());
    } else {
        println!();
        println!("  {}", "LIVE EXECUTION AUDIT".bold().cyan());
        println!();
        println!("  This tool analyzes FillLog CSVs from live or backtest trading.");
        println!();
        println!("  Usage:");
        println!();
        println!("  {:>4}  {}  Analyze live FillLog CSVs", "", "--csv data/cache/fill_logs/");
        println!("  {:>4}  {}  Generate + analyze synthetic backtest fills", "", "--simulate-backtest");
        println!();
        println!("  {}  Run with --simulate-backtest to test the pipeline.", "💡".yellow());
        println!();
        // Default: run synthetic
        println!("  Running synthetic backtest simulation by default...");
        let out_path = PathBuf::from("data/cache/synthetic_fill_log.csv");
        std::fs::create_dir_all(out_path.parent().unwrap())?;
        records = generate_backtest_fill_log(&out_path)?;
        let (stats, alerts) = analyze_fills(&records);
        print_report(&stats, &alerts, &format!("synthetic backtest ({})", out_path.display()));
    }

    Ok(())
}
