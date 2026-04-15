//! Integrated execution-trust summary across the recent daily-harness audits.
//!
//! Purpose:
//! - stop carrying fee, latency, and passive-entry results as separate stories
//! - summarize what each audit actually says about trust, not just level uplift
//! - keep the output auditable by reading the latest snapshot CSV artifacts
//!
//! Inputs:
//! - snapshots/trend_portfolio_latency_latest.csv
//! - snapshots/trend_fee_surface_latest.csv
//! - snapshots/oldguard_passive_entry_latest.csv
//!
//! Outputs:
//! - snapshots/execution_trust_summary_latest.md
//! - snapshots/execution_trust_summary_latest.csv
//! - snapshots/execution_trust_summary_<UTC timestamp>.md
//! - snapshots/execution_trust_summary_<UTC timestamp>.csv

use anyhow::{anyhow, Result};
use chrono::Utc;
use std::{collections::HashMap, fs, path::Path};

const LATENCY_CSV: &str = "snapshots/trend_portfolio_latency_latest.csv";
const FEE_CSV: &str = "snapshots/trend_fee_surface_latest.csv";
const PASSIVE_CSV: &str = "snapshots/oldguard_passive_entry_latest.csv";
const SNAPSHOT_DIR: &str = "snapshots";
const LATEST_MD: &str = "snapshots/execution_trust_summary_latest.md";
const LATEST_CSV: &str = "snapshots/execution_trust_summary_latest.csv";

const TRACKED_STRATEGIES: &[&str] = &[
    "MACD+Regime",
    "Turtle+Regime+MACD",
    "Ensemble(Majority 2/3)",
    "CrossSectionalMomentum",
    "MACD",
];

#[derive(Debug, Clone, Default)]
struct LatencyRow {
    score: f64,
    baseline_wins: usize,
    delay_wins: usize,
    baseline_resamples: usize,
    baseline_resamples_total: usize,
    delay_resamples: usize,
    delay_resamples_total: usize,
    avg_baseline_sharpe: f64,
    avg_delay_sharpe: f64,
    avg_delay_return_delta_pct: f64,
}

#[derive(Debug, Clone, Default)]
struct FeeAggregate {
    universes: usize,
    taker_wf_passed: usize,
    taker_wf_total: usize,
    taker_resamples: usize,
    taker_resamples_total: usize,
    maker_entry_improved_resamples: usize,
    maker_entry_improved_wf: usize,
    maker_maker_improved_resamples: usize,
    maker_maker_improved_wf: usize,
    taker_return_sum: f64,
    maker_entry_return_sum: f64,
    maker_maker_return_sum: f64,
}

#[derive(Debug, Clone, Default)]
struct PassiveRow {
    taker_return_pct: f64,
    passive_return_pct: f64,
    taker_wf_passed: usize,
    passive_wf_passed: usize,
    taker_resamples: usize,
    passive_resamples: usize,
    fill_rate_pct: f64,
    avg_entry_edge_bps: f64,
}

#[derive(Debug, Clone)]
struct SummaryRow {
    strategy: String,
    latency_score: Option<f64>,
    latency_baseline_wins: Option<usize>,
    latency_delay_wins: Option<usize>,
    latency_baseline_resamples: Option<String>,
    latency_delay_resamples: Option<String>,
    latency_avg_baseline_sharpe: Option<f64>,
    latency_avg_delay_sharpe: Option<f64>,
    latency_avg_delay_return_delta_pct: Option<f64>,
    fee_taker_avg_return_pct: Option<f64>,
    fee_maker_entry_avg_return_pct: Option<f64>,
    fee_maker_maker_avg_return_pct: Option<f64>,
    fee_taker_wf: Option<String>,
    fee_taker_resamples: Option<String>,
    fee_maker_entry_improved_wf: Option<usize>,
    fee_maker_entry_improved_resamples: Option<usize>,
    fee_maker_maker_improved_wf: Option<usize>,
    fee_maker_maker_improved_resamples: Option<usize>,
    passive_taker_return_pct: Option<f64>,
    passive_return_pct: Option<f64>,
    passive_taker_wf: Option<String>,
    passive_wf: Option<String>,
    passive_taker_resamples: Option<String>,
    passive_resamples: Option<String>,
    passive_fill_rate_pct: Option<f64>,
    passive_avg_entry_edge_bps: Option<f64>,
    trust_read: String,
}

fn main() -> Result<()> {
    if !Path::new(LATENCY_CSV).exists()
        || !Path::new(FEE_CSV).exists()
        || !Path::new(PASSIVE_CSV).exists()
    {
        return Err(anyhow!(
            "expected latest latency / fee / passive snapshot CSVs to exist"
        ));
    }

    let latency = read_latency_rows(LATENCY_CSV)?;
    let fee = read_fee_rows(FEE_CSV)?;
    let passive = read_passive_rows(PASSIVE_CSV)?;

    let mut rows = Vec::new();
    for &strategy in TRACKED_STRATEGIES {
        let latency_row = latency.get(strategy);
        let fee_row = fee.get(strategy);
        let passive_row = passive.get(strategy);

        rows.push(SummaryRow {
            strategy: strategy.to_string(),
            latency_score: latency_row.map(|r| r.score),
            latency_baseline_wins: latency_row.map(|r| r.baseline_wins),
            latency_delay_wins: latency_row.map(|r| r.delay_wins),
            latency_baseline_resamples: latency_row
                .map(|r| format!("{}/{}", r.baseline_resamples, r.baseline_resamples_total)),
            latency_delay_resamples: latency_row
                .map(|r| format!("{}/{}", r.delay_resamples, r.delay_resamples_total)),
            latency_avg_baseline_sharpe: latency_row.map(|r| r.avg_baseline_sharpe),
            latency_avg_delay_sharpe: latency_row.map(|r| r.avg_delay_sharpe),
            latency_avg_delay_return_delta_pct: latency_row.map(|r| r.avg_delay_return_delta_pct),
            fee_taker_avg_return_pct: fee_row.map(|r| r.taker_return_sum / r.universes as f64),
            fee_maker_entry_avg_return_pct: fee_row
                .map(|r| r.maker_entry_return_sum / r.universes as f64),
            fee_maker_maker_avg_return_pct: fee_row
                .map(|r| r.maker_maker_return_sum / r.universes as f64),
            fee_taker_wf: fee_row.map(|r| format!("{}/{}", r.taker_wf_passed, r.taker_wf_total)),
            fee_taker_resamples: fee_row
                .map(|r| format!("{}/{}", r.taker_resamples, r.taker_resamples_total)),
            fee_maker_entry_improved_wf: fee_row.map(|r| r.maker_entry_improved_wf),
            fee_maker_entry_improved_resamples: fee_row.map(|r| r.maker_entry_improved_resamples),
            fee_maker_maker_improved_wf: fee_row.map(|r| r.maker_maker_improved_wf),
            fee_maker_maker_improved_resamples: fee_row.map(|r| r.maker_maker_improved_resamples),
            passive_taker_return_pct: passive_row.map(|r| r.taker_return_pct),
            passive_return_pct: passive_row.map(|r| r.passive_return_pct),
            passive_taker_wf: passive_row.map(|r| format!("{}/4", r.taker_wf_passed)),
            passive_wf: passive_row.map(|r| format!("{}/4", r.passive_wf_passed)),
            passive_taker_resamples: passive_row.map(|r| format!("{}/15", r.taker_resamples)),
            passive_resamples: passive_row.map(|r| format!("{}/15", r.passive_resamples)),
            passive_fill_rate_pct: passive_row.map(|r| r.fill_rate_pct),
            passive_avg_entry_edge_bps: passive_row.map(|r| r.avg_entry_edge_bps),
            trust_read: trust_read(strategy, latency_row, fee_row, passive_row),
        });
    }

    write_outputs(&rows)?;
    print_console_summary(&rows);
    Ok(())
}

fn read_latency_rows(path: &str) -> Result<HashMap<String, LatencyRow>> {
    let content = fs::read_to_string(path)?;
    let mut out = HashMap::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split(',').collect();
        if cols.get(0) != Some(&"integrated") {
            continue;
        }
        let strategy = cols
            .get(5)
            .ok_or_else(|| anyhow!("bad latency row"))?
            .to_string();
        out.insert(
            strategy,
            LatencyRow {
                score: parse_f64(cols.get(6))?,
                baseline_wins: parse_usize(cols.get(7))?,
                delay_wins: parse_usize(cols.get(9))?,
                baseline_resamples: parse_usize(cols.get(14))?,
                baseline_resamples_total: parse_usize(cols.get(15))?,
                delay_resamples: parse_usize(cols.get(16))?,
                delay_resamples_total: parse_usize(cols.get(17))?,
                avg_baseline_sharpe: parse_f64(cols.get(20))?,
                avg_delay_sharpe: parse_f64(cols.get(21))?,
                avg_delay_return_delta_pct: parse_f64(cols.get(22))?,
            },
        );
    }
    Ok(out)
}

fn read_fee_rows(path: &str) -> Result<HashMap<String, FeeAggregate>> {
    let content = fs::read_to_string(path)?;
    let mut per_strategy_universe: HashMap<
        (String, String),
        HashMap<String, (usize, usize, usize, usize, f64)>,
    > = HashMap::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split(',').collect();
        let universe = cols
            .get(0)
            .ok_or_else(|| anyhow!("bad fee row"))?
            .to_string();
        let strategy = cols
            .get(1)
            .ok_or_else(|| anyhow!("bad fee row"))?
            .to_string();
        let scenario = cols
            .get(2)
            .ok_or_else(|| anyhow!("bad fee row"))?
            .to_string();
        let wf_passed = parse_usize(cols.get(6))?;
        let wf_total = parse_usize(cols.get(7))?;
        let rs_passed = parse_usize(cols.get(8))?;
        let rs_total = parse_usize(cols.get(9))?;
        let full_return = parse_f64(cols.get(3))?;
        per_strategy_universe
            .entry((strategy, universe))
            .or_default()
            .insert(
                scenario,
                (wf_passed, wf_total, rs_passed, rs_total, full_return),
            );
    }

    let mut out = HashMap::new();
    for ((strategy, _universe), scenarios) in per_strategy_universe {
        let taker = scenarios.get("Taker/Taker (10+10 bps)");
        let maker_entry = scenarios.get("MakerEntry/TakerExit (2+10 bps)");
        let maker_maker = scenarios.get("Maker/Maker (2+2 bps)");
        let Some(&(t_wf, t_wf_total, t_rs, t_rs_total, t_ret)) = taker else {
            continue;
        };
        let agg = out.entry(strategy).or_insert_with(FeeAggregate::default);
        agg.universes += 1;
        agg.taker_wf_passed += t_wf;
        agg.taker_wf_total += t_wf_total;
        agg.taker_resamples += t_rs;
        agg.taker_resamples_total += t_rs_total;
        agg.taker_return_sum += t_ret;
        if let Some(&(wf, _, rs, _, ret)) = maker_entry {
            agg.maker_entry_return_sum += ret;
            if wf > t_wf {
                agg.maker_entry_improved_wf += 1;
            }
            if rs > t_rs {
                agg.maker_entry_improved_resamples += 1;
            }
        }
        if let Some(&(wf, _, rs, _, ret)) = maker_maker {
            agg.maker_maker_return_sum += ret;
            if wf > t_wf {
                agg.maker_maker_improved_wf += 1;
            }
            if rs > t_rs {
                agg.maker_maker_improved_resamples += 1;
            }
        }
    }
    Ok(out)
}

fn read_passive_rows(path: &str) -> Result<HashMap<String, PassiveRow>> {
    let content = fs::read_to_string(path)?;
    let mut temp: HashMap<String, PassiveRow> = HashMap::new();
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split(',').collect();
        let strategy = cols
            .get(0)
            .ok_or_else(|| anyhow!("bad passive row"))?
            .to_string();

        let (scenario, offset) = if cols.get(1) == Some(&"PassiveEntry/TakerExit (real fill") {
            ("PassiveEntry/TakerExit (real fill, 2+10 bps)", 1usize)
        } else {
            (
                cols.get(1)
                    .ok_or_else(|| anyhow!("bad passive row"))?
                    .trim(),
                0usize,
            )
        };

        let row = temp.entry(strategy).or_default();
        if scenario == "Taker/Taker (10+10 bps)" {
            row.taker_return_pct = parse_f64(cols.get(2 + offset))?;
            row.taker_wf_passed = parse_usize(cols.get(9 + offset))?;
            row.taker_resamples = parse_usize(cols.get(11 + offset))?;
        } else {
            row.passive_return_pct = parse_f64(cols.get(2 + offset))?;
            row.passive_wf_passed = parse_usize(cols.get(9 + offset))?;
            row.passive_resamples = parse_usize(cols.get(11 + offset))?;
            row.fill_rate_pct = parse_f64(cols.get(5 + offset))?;
            row.avg_entry_edge_bps = parse_f64(cols.get(7 + offset))?;
        }
    }
    Ok(temp)
}

fn trust_read(
    strategy: &str,
    latency: Option<&LatencyRow>,
    fee: Option<&FeeAggregate>,
    passive: Option<&PassiveRow>,
) -> String {
    let latency_text = match latency {
        Some(l) if l.avg_delay_return_delta_pct < -50000.0 => "latency-fragile",
        Some(l) if l.delay_resamples < l.baseline_resamples => "delay hurts chronology",
        Some(l) => {
            if l.delay_wins > l.baseline_wins {
                "delay-level help, chronology still mixed"
            } else {
                "moderately delay-stable"
            }
        }
        None => "no latency read",
    };

    let fee_text = match fee {
        Some(f)
            if f.maker_entry_improved_resamples == 0 && f.maker_maker_improved_resamples == 0 =>
        {
            "fees mostly lift level only"
        }
        Some(f) if f.maker_maker_improved_resamples > 0 || f.maker_entry_improved_resamples > 0 => {
            "fees can change borderline verdicts"
        }
        None => "no fee read",
        _ => "mixed fee read",
    };

    let passive_text = match passive {
        Some(p)
            if p.passive_resamples < p.taker_resamples
                || p.passive_return_pct < p.taker_return_pct =>
        {
            "real passive entry did not confirm uplift"
        }
        Some(p) if p.avg_entry_edge_bps > 0.0 => "candidate-specific passive help",
        Some(_) => "passive neutral",
        None => "no passive read",
    };

    match strategy {
        "MACD+Regime" => format!("{}; {}; {}", latency_text, fee_text, passive_text),
        "Turtle+Regime+MACD" => format!("{}; {}", latency_text, fee_text),
        "Ensemble(Majority 2/3)" | "CrossSectionalMomentum" => {
            format!("{}; {}", fee_text, passive_text)
        }
        _ => format!("{}; {}", latency_text, fee_text),
    }
}

fn write_outputs(rows: &[SummaryRow]) -> Result<()> {
    fs::create_dir_all(SNAPSHOT_DIR)?;
    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    let archive_md = format!("{}/execution_trust_summary_{}.md", SNAPSHOT_DIR, timestamp);
    let archive_csv = format!("{}/execution_trust_summary_{}.csv", SNAPSHOT_DIR, timestamp);

    let md = render_markdown(rows);
    let csv = render_csv(rows);

    fs::write(LATEST_MD, &md)?;
    fs::write(LATEST_CSV, &csv)?;
    fs::write(&archive_md, md)?;
    fs::write(&archive_csv, csv)?;

    println!("Wrote:");
    println!("- {}", LATEST_MD);
    println!("- {}", LATEST_CSV);
    println!("- {}", archive_md);
    println!("- {}", archive_csv);
    Ok(())
}

fn render_markdown(rows: &[SummaryRow]) -> String {
    let mut out = String::new();
    out.push_str("# Execution Trust Summary\n\n");
    out.push_str("Integrated read across the latest latency, fee-surface, and old-guard passive-entry audits.\n\n");
    out.push_str("## High-level read\n\n");
    out.push_str("- Latency remains the biggest trust separator for the main trend leaders.\n");
    out.push_str("- Lower fees usually lift returns more than they improve chronology.\n");
    out.push_str("- Real passive-entry fills did not broadly confirm the old-guard fee-surface rescue story.\n\n");
    out.push_str("## Strategy summary\n\n");
    out.push_str("| Strategy | Latency | Fee surface | Passive entry | Trust read |\n");
    out.push_str("|---|---:|---:|---:|---|\n");
    for row in rows {
        let latency = row
            .latency_score
            .map(|v| format!("score {:.3}", v))
            .unwrap_or_else(|| "n/a".to_string());
        let fee = match (
            row.fee_taker_resamples.as_ref(),
            row.fee_maker_entry_improved_resamples,
            row.fee_maker_maker_improved_resamples,
        ) {
            (Some(base), Some(h), Some(m)) => format!("base {} | +hyb {}u | +mm {}u", base, h, m),
            _ => "n/a".to_string(),
        };
        let passive = match (
            row.passive_taker_resamples.as_ref(),
            row.passive_resamples.as_ref(),
            row.passive_avg_entry_edge_bps,
        ) {
            (Some(t), Some(p), Some(edge)) => format!("{} -> {} | edge {:+.1}bps", t, p, edge),
            _ => "n/a".to_string(),
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            row.strategy, latency, fee, passive, row.trust_read
        ));
    }
    out.push_str("\n## Notes\n\n");
    out.push_str("- `+hyb` / `+mm` count how many fee-surface universes improved chronology under MakerEntry/TakerExit or Maker/Maker versus Taker/Taker.\n");
    out.push_str("- Passive-entry read is only available for the OldGuardNoBNB audit.\n");
    out.push_str("- Absence of a row in one audit means that family was not part of that particular test, not that it passed or failed.\n");
    out
}

fn render_csv(rows: &[SummaryRow]) -> String {
    let mut out = String::from("strategy,latency_score,latency_baseline_wins,latency_delay_wins,latency_baseline_resamples,latency_delay_resamples,latency_avg_baseline_sharpe,latency_avg_delay_sharpe,latency_avg_delay_return_delta_pct,fee_taker_avg_return_pct,fee_maker_entry_avg_return_pct,fee_maker_maker_avg_return_pct,fee_taker_wf,fee_taker_resamples,fee_maker_entry_improved_wf,fee_maker_entry_improved_resamples,fee_maker_maker_improved_wf,fee_maker_maker_improved_resamples,passive_taker_return_pct,passive_return_pct,passive_taker_wf,passive_wf,passive_taker_resamples,passive_resamples,passive_fill_rate_pct,passive_avg_entry_edge_bps,trust_read\n");
    for r in rows {
        out.push_str(&format!(
            "{} ,{} ,{} ,{} ,{} ,{} ,{} ,{} ,{} ,{} ,{} ,{} ,{} ,{} ,{} ,{} ,{} ,{} ,{} ,{} ,{} ,{} ,{} ,{} ,{} ,{} ,{}\n",
            r.strategy,
            optf(r.latency_score),
            optu(r.latency_baseline_wins),
            optu(r.latency_delay_wins),
            opts(r.latency_baseline_resamples.as_ref()),
            opts(r.latency_delay_resamples.as_ref()),
            optf(r.latency_avg_baseline_sharpe),
            optf(r.latency_avg_delay_sharpe),
            optf(r.latency_avg_delay_return_delta_pct),
            optf(r.fee_taker_avg_return_pct),
            optf(r.fee_maker_entry_avg_return_pct),
            optf(r.fee_maker_maker_avg_return_pct),
            opts(r.fee_taker_wf.as_ref()),
            opts(r.fee_taker_resamples.as_ref()),
            optu(r.fee_maker_entry_improved_wf),
            optu(r.fee_maker_entry_improved_resamples),
            optu(r.fee_maker_maker_improved_wf),
            optu(r.fee_maker_maker_improved_resamples),
            optf(r.passive_taker_return_pct),
            optf(r.passive_return_pct),
            opts(r.passive_taker_wf.as_ref()),
            opts(r.passive_wf.as_ref()),
            opts(r.passive_taker_resamples.as_ref()),
            opts(r.passive_resamples.as_ref()),
            optf(r.passive_fill_rate_pct),
            optf(r.passive_avg_entry_edge_bps),
            r.trust_read.replace(',', ";")
        ));
    }
    out
}

fn print_console_summary(rows: &[SummaryRow]) {
    println!("=== EXECUTION TRUST SUMMARY ===\n");
    for row in rows {
        println!("{}", row.strategy);
        if let Some(score) = row.latency_score {
            println!(
                "  latency score {:.3} | baseline wins {} | delay wins {} | base RS {} | delay RS {}",
                score,
                row.latency_baseline_wins.unwrap_or(0),
                row.latency_delay_wins.unwrap_or(0),
                row.latency_baseline_resamples.as_deref().unwrap_or("n/a"),
                row.latency_delay_resamples.as_deref().unwrap_or("n/a")
            );
        }
        if let Some(base) = row.fee_taker_avg_return_pct {
            println!(
                "  fee avg ret {:.1}% -> {:.1}% -> {:.1}% | resample improvements hyb {} | mm {}",
                base,
                row.fee_maker_entry_avg_return_pct.unwrap_or(base),
                row.fee_maker_maker_avg_return_pct.unwrap_or(base),
                row.fee_maker_entry_improved_resamples.unwrap_or(0),
                row.fee_maker_maker_improved_resamples.unwrap_or(0)
            );
        }
        if let Some(passive) = row.passive_return_pct {
            println!(
                "  passive oldguard {:.1}% -> {:.1}% | RS {} -> {} | edge {:+.1}bps | fill {:.1}%",
                row.passive_taker_return_pct.unwrap_or(0.0),
                passive,
                row.passive_taker_resamples.as_deref().unwrap_or("n/a"),
                row.passive_resamples.as_deref().unwrap_or("n/a"),
                row.passive_avg_entry_edge_bps.unwrap_or(0.0),
                row.passive_fill_rate_pct.unwrap_or(0.0)
            );
        }
        println!("  read: {}\n", row.trust_read);
    }
}

fn parse_f64(v: Option<&&str>) -> Result<f64> {
    Ok(v.ok_or_else(|| anyhow!("missing f64"))?.trim().parse()?)
}
fn parse_usize(v: Option<&&str>) -> Result<usize> {
    Ok(v.ok_or_else(|| anyhow!("missing usize"))?.trim().parse()?)
}
fn optf(v: Option<f64>) -> String {
    v.map(|x| format!("{:.4}", x)).unwrap_or_default()
}
fn optu(v: Option<usize>) -> String {
    v.map(|x| x.to_string()).unwrap_or_default()
}
fn opts(v: Option<&String>) -> String {
    v.cloned().unwrap_or_default()
}
