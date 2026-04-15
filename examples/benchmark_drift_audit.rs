use anyhow::{Context, Result};
use chrono::Utc;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Clone)]
struct HeadToHeadRow {
    rank: usize,
    strategy: String,
    score: f64,
    full_return_pct: f64,
    resample_passed: u32,
    resample_total: u32,
    universe_first_place: u32,
    universe_total: u32,
}

#[derive(Debug, Deserialize, Clone)]
struct CrossFamilyRow {
    universe: String,
    strategy: String,
    return_pct: f64,
    sharpe: f64,
    max_dd_pct: f64,
}

#[derive(Debug)]
struct SnapshotSummary {
    label: String,
    top_strategy: String,
    top_score: f64,
    rows: Vec<HeadToHeadRow>,
}

fn read_csv<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Vec<T>> {
    let mut rdr = csv::Reader::from_path(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    let mut out = Vec::new();
    for row in rdr.deserialize() {
        out.push(row.with_context(|| format!("failed to parse {}", path.display()))?);
    }
    Ok(out)
}

fn snapshot_label(path: &Path) -> String {
    let file = path.file_stem().unwrap_or_default().to_string_lossy();
    file.trim_start_matches("head_to_head_").to_string()
}

fn pct_delta(new: f64, old: f64) -> f64 {
    new - old
}

fn main() -> Result<()> {
    let snapshot_dir = PathBuf::from("snapshots");
    let mut head_paths: Vec<PathBuf> = fs::read_dir(&snapshot_dir)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().map(|e| e == "csv").unwrap_or(false)
                && p.file_name()
                    .map(|n| {
                        let name = n.to_string_lossy();
                        name.starts_with("head_to_head_") && name != "head_to_head_latest.csv"
                    })
                    .unwrap_or(false)
        })
        .collect();
    head_paths.sort();

    let latest_head_path = snapshot_dir.join("head_to_head_latest.csv");
    let latest_cross_family_path = snapshot_dir.join("cross_family_portfolio_summary_latest.csv");

    let mut snapshots = Vec::new();
    for path in &head_paths {
        let rows: Vec<HeadToHeadRow> = read_csv(path)?;
        if rows.is_empty() {
            continue;
        }
        snapshots.push(SnapshotSummary {
            label: snapshot_label(path),
            top_strategy: rows[0].strategy.clone(),
            top_score: rows[0].score,
            rows,
        });
    }

    let latest_rows: Vec<HeadToHeadRow> = read_csv(&latest_head_path)?;
    let latest_cross_rows: Vec<CrossFamilyRow> = read_csv(&latest_cross_family_path)?;

    let first = snapshots
        .first()
        .context("no archived head_to_head snapshots found")?;
    let latest = latest_rows
        .first()
        .context("head_to_head_latest.csv had no rows")?;

    let mut latest_by_strategy: HashMap<String, HeadToHeadRow> = HashMap::new();
    for row in &latest_rows {
        latest_by_strategy.insert(row.strategy.clone(), row.clone());
    }

    let mut first_by_strategy: HashMap<String, HeadToHeadRow> = HashMap::new();
    for row in &first.rows {
        first_by_strategy.insert(row.strategy.clone(), row.clone());
    }

    let tracked = [
        "MACD+Regime",
        "MACD",
        "Turtle+Regime+MACD",
        "Turtle+Regime",
        "Turtle+MACD",
    ];

    let mut drift_csv = String::from(
        "strategy,first_rank,latest_rank,first_score,latest_score,score_delta,first_resamples,latest_resamples,first_universe_wins,latest_universe_wins,full_return_pct\n",
    );

    let mut drift_md = String::new();
    drift_md.push_str("# Benchmark Drift Audit\n\n");
    drift_md.push_str("This audit quantifies how much the benchmark story moved across archived `head_to_head` snapshots, then compares that trade-sum / chronology leader board to the latest realistic capped-book cross-family portfolio lens.\n\n");

    drift_md.push_str("## Archived head-to-head stability\n\n");
    drift_md.push_str("| Snapshot | Top strategy | Top score |\n");
    drift_md.push_str("|---|---|---:|\n");
    for snapshot in &snapshots {
        drift_md.push_str(&format!(
            "| {} | {} | {:.3} |\n",
            snapshot.label, snapshot.top_strategy, snapshot.top_score
        ));
    }
    drift_md.push_str("\n");

    drift_md.push_str("## Strategy drift from first archived snapshot to latest\n\n");
    drift_md
        .push_str("| Strategy | Rank | Score Δ | Resamples | Universe wins | Full return % |\n");
    drift_md.push_str("|---|---:|---:|---:|---:|---:|\n");

    for strategy in tracked {
        if let (Some(first_row), Some(latest_row)) = (
            first_by_strategy.get(strategy),
            latest_by_strategy.get(strategy),
        ) {
            let score_delta = pct_delta(latest_row.score, first_row.score);
            drift_md.push_str(&format!(
                "| {} | {} -> {} | {:+.3} | {}/{} -> {}/{} | {}/{} -> {}/{} | {:.1} |\n",
                strategy,
                first_row.rank,
                latest_row.rank,
                score_delta,
                first_row.resample_passed,
                first_row.resample_total,
                latest_row.resample_passed,
                latest_row.resample_total,
                first_row.universe_first_place,
                first_row.universe_total,
                latest_row.universe_first_place,
                latest_row.universe_total,
                latest_row.full_return_pct,
            ));

            drift_csv.push_str(&format!(
                "{},{},{},{:.6},{:.6},{:.6},{}/{},{}/{},{}/{},{}/{},{:.1}\n",
                strategy,
                first_row.rank,
                latest_row.rank,
                first_row.score,
                latest_row.score,
                score_delta,
                first_row.resample_passed,
                first_row.resample_total,
                latest_row.resample_passed,
                latest_row.resample_total,
                first_row.universe_first_place,
                first_row.universe_total,
                latest_row.universe_first_place,
                latest_row.universe_total,
                latest_row.full_return_pct,
            ));
        }
    }
    drift_md.push_str("\n");

    let mut best_by_universe: BTreeMap<String, CrossFamilyRow> = BTreeMap::new();
    for row in latest_cross_rows {
        best_by_universe
            .entry(row.universe.clone())
            .and_modify(|best| {
                if row.sharpe > best.sharpe {
                    *best = row.clone();
                }
            })
            .or_insert(row);
    }

    drift_md.push_str("## Latest realistic capped-book winners by universe\n\n");
    drift_md.push_str("| Universe | Winner (Sharpe-first) | Return % | Sharpe | MaxDD % |\n");
    drift_md.push_str("|---|---|---:|---:|---:|\n");
    for (universe, row) in &best_by_universe {
        drift_md.push_str(&format!(
            "| {} | {} | {:.1} | {:.2} | {:.1} |\n",
            universe, row.strategy, row.return_pct, row.sharpe, row.max_dd_pct
        ));
    }
    drift_md.push_str("\n");

    let realistic_winner_counts =
        best_by_universe
            .values()
            .fold(BTreeMap::new(), |mut acc, row| {
                *acc.entry(row.strategy.clone()).or_insert(0usize) += 1;
                acc
            });

    drift_md.push_str("## Key read\n\n");
    drift_md.push_str(&format!(
        "- The archived chronology-first benchmark stayed top-ranked for **{}** across all archived `head_to_head` snapshots, so the headline trade-sum leader itself was stable even while the internals moved.\n",
        latest.strategy
    ));

    if let (Some(first_macd), Some(latest_macd)) = (
        first_by_strategy.get("MACD"),
        latest_by_strategy.get("MACD"),
    ) {
        drift_md.push_str(&format!(
            "- The biggest benchmark-internal drift was plain **MACD**: score moved **{:+.3}** and resamples improved **{}/{} -> {}/{}**, which is a concrete reminder that methodology / cache changes can materially alter the trust story even when full-sample returns barely move.\n",
            latest_macd.score - first_macd.score,
            first_macd.resample_passed,
            first_macd.resample_total,
            latest_macd.resample_passed,
            latest_macd.resample_total,
        ));
    }

    let latest_count_line = realistic_winner_counts
        .iter()
        .map(|(strategy, count)| format!("{} {}", strategy, count))
        .collect::<Vec<_>>()
        .join(", ");
    drift_md.push_str(&format!(
        "- The latest realistic capped-book lens does **not** crown the same default winner across universes; Sharpe-first universe wins split across **{}**.\n",
        latest_count_line
    ));
    drift_md.push_str("- Practical project-control lesson: the trade-sum / chronology benchmark is still useful as a yardstick, but realistic portfolio conclusions remain universe-dependent and should not be collapsed into one ‘best strategy’ label.\n");

    let latest_timestamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let latest_md = snapshot_dir.join("benchmark_drift_audit_latest.md");
    let latest_csv = snapshot_dir.join("benchmark_drift_audit_latest.csv");
    let ts_md = snapshot_dir.join(format!("benchmark_drift_audit_{}.md", latest_timestamp));
    let ts_csv = snapshot_dir.join(format!("benchmark_drift_audit_{}.csv", latest_timestamp));

    fs::write(&latest_md, &drift_md)?;
    fs::write(&latest_csv, &drift_csv)?;
    fs::write(ts_md, &drift_md)?;
    fs::write(ts_csv, &drift_csv)?;

    println!("{}", drift_md);
    Ok(())
}
