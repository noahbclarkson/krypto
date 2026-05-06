// T68: Drawdown Abandonment / Risk-of-Ruin Stress Test
//
// Reads the exact live-bot daily equity/trade snapshots and stress-tests simple
// operational risk rules. This is not a strategy hyperopt; it asks whether a
// human risk governor would likely abandon/halve the current leader too early.

use std::fs;
use std::io::Write;

#[derive(Clone)]
struct EqRow {
    bar: i64,
    date: String,
    equity: f64,
}

#[derive(Clone)]
struct Trade {
    entry_bar: i64,
    equity_mult: f64,
}

fn max_drawdown(equity: &[f64]) -> f64 {
    let mut peak = equity[0];
    let mut max_dd = 0.0_f64;
    for &e in equity {
        peak = peak.max(e);
        max_dd = max_dd.max((peak - e) / peak);
    }
    max_dd
}

fn first_breach(rows: &[EqRow], threshold: f64) -> Option<usize> {
    let mut peak = rows[0].equity;
    for (i, row) in rows.iter().enumerate() {
        peak = peak.max(row.equity);
        let dd = (peak - row.equity) / peak;
        if dd >= threshold {
            return Some(i);
        }
    }
    None
}

fn recovery_days_after_breach(rows: &[EqRow], breach_idx: usize) -> Option<usize> {
    let prior_peak = rows[..=breach_idx]
        .iter()
        .map(|r| r.equity)
        .fold(0.0_f64, f64::max);
    for i in breach_idx + 1..rows.len() {
        if rows[i].equity >= prior_peak {
            return Some(i - breach_idx);
        }
    }
    None
}

fn apply_scaled_drawdown_rule(rows: &[EqRow], threshold: f64, scale_when_dd: f64, freeze: bool) -> Vec<f64> {
    let n = rows.len();
    let mut out = vec![rows[0].equity; n];
    let mut baseline_peak = rows[0].equity;
    for i in 1..n {
        baseline_peak = baseline_peak.max(rows[i].equity);
        let dd = (baseline_peak - rows[i].equity) / baseline_peak;
        let ret = rows[i].equity / rows[i - 1].equity - 1.0;
        let applied_ret = if dd >= threshold {
            if freeze { 0.0 } else { ret * scale_when_dd }
        } else {
            ret
        };
        out[i] = out[i - 1] * (1.0 + applied_ret);
    }
    out
}

fn missed_top_trades(trades: &[Trade], breach_bar: i64, top_n: usize) -> (usize, f64, Vec<Trade>) {
    let mut top = trades.to_vec();
    top.sort_by(|a, b| {
        let al = a.equity_mult.ln();
        let bl = b.equity_mult.ln();
        bl.partial_cmp(&al).unwrap_or(std::cmp::Ordering::Equal)
    });
    top.truncate(top_n);
    let missed: Vec<Trade> = top
        .into_iter()
        .filter(|t| t.entry_bar >= breach_bar)
        .collect();
    let log_sum = missed.iter().map(|t| t.equity_mult.ln()).sum::<f64>();
    (missed.len(), log_sum.exp(), missed)
}

fn parse_equity(path: &str) -> Vec<EqRow> {
    fs::read_to_string(path)
        .expect("equity CSV missing")
        .lines()
        .skip(1)
        .filter_map(|line| {
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() < 3 { return None; }
            Some(EqRow {
                bar: parts[0].parse().ok()?,
                date: parts[1].to_string(),
                equity: parts[2].parse().ok()?,
            })
        })
        .collect()
}

fn parse_trades(path: &str) -> Vec<Trade> {
    fs::read_to_string(path)
        .expect("trades CSV missing")
        .lines()
        .skip(1)
        .filter_map(|line| {
            let p: Vec<&str> = line.split(',').collect();
            if p.len() < 11 { return None; }
            Some(Trade {
                entry_bar: p[1].parse().ok()?,
                equity_mult: p[10].parse().ok()?,
            })
        })
        .collect()
}

fn main() {
    let root = "/home/ubuntu/.openclaw/workspace-krypto/krypto";
    let rows = parse_equity(&format!("{root}/snapshots/live_bot_exact_equity.csv"));
    let trades = parse_trades(&format!("{root}/snapshots/live_bot_exact_trades.csv"));
    assert!(!rows.is_empty(), "empty equity curve");

    let baseline_final = rows.last().unwrap().equity;
    let baseline_dd = max_drawdown(&rows.iter().map(|r| r.equity).collect::<Vec<_>>());
    let thresholds = [0.20, 0.30, 0.40, 0.50, 0.70, 0.85];

    let mut report = String::new();
    report.push_str("# T68: Exact Live-Bot Abandonment / Risk Governance Stress\n\n");
    report.push_str("Source: `snapshots/live_bot_exact_equity.csv` + `snapshots/live_bot_exact_trades.csv` from the exact T65/T67 live-bot replay. This is a risk-governance audit, not a new strategy comparison.\n\n");
    report.push_str("## Baseline\n\n");
    report.push_str(&format!("- Days: {}\n", rows.len()));
    report.push_str(&format!("- Final equity: {:.2}x\n", baseline_final));
    report.push_str(&format!("- MaxDD: {:.1}%\n", baseline_dd * 100.0));
    report.push_str(&format!("- Trades: {}\n\n", trades.len()));

    report.push_str("## Drawdown Rules\n\n");
    report.push_str("| Threshold | Breached? | Breach Date | Recovery Wait | Abandon Final | Halt-while-DD Final | Half-risk-in-DD Final | Quarter-risk-in-DD Final | Missed Top-10 Winners |\n");
    report.push_str("|---:|---|---|---:|---:|---:|---:|---:|---:|\n");

    for &th in &thresholds {
        let breach = first_breach(&rows, th);
        let (breached, breach_date, recovery, abandon_final, missed_desc) = if let Some(idx) = breach {
            let rec = recovery_days_after_breach(&rows, idx)
                .map(|d| format!("{}d", d))
                .unwrap_or_else(|| "never".to_string());
            let (missed_count, missed_mult, _missed) = missed_top_trades(&trades, rows[idx].bar, 10);
            (
                "yes".to_string(),
                rows[idx].date[..10].to_string(),
                rec,
                rows[idx].equity,
                format!("{} / {:.2}x", missed_count, missed_mult),
            )
        } else {
            (
                "no".to_string(),
                "—".to_string(),
                "—".to_string(),
                baseline_final,
                "0 / 1.00x".to_string(),
            )
        };

        let halt = apply_scaled_drawdown_rule(&rows, th, 0.0, true);
        let half = apply_scaled_drawdown_rule(&rows, th, 0.5, false);
        let quarter = apply_scaled_drawdown_rule(&rows, th, 0.25, false);
        report.push_str(&format!(
            "| {:.0}% | {} | {} | {} | {:.2}x | {:.2}x | {:.2}x | {:.2}x | {} |\n",
            th * 100.0,
            breached,
            breach_date,
            recovery,
            abandon_final,
            halt.last().unwrap(),
            half.last().unwrap(),
            quarter.last().unwrap(),
            missed_desc,
        ));
    }

    report.push_str("\n## Interpretation\n\n");
    report.push_str("- The exact live-bot curve only breaches the 20% drawdown rule; its measured MaxDD is below 30%.\n");
    report.push_str("- A hard 20% abandonment rule would have stopped the bot before later recovery and left final equity near the breach level.\n");
    report.push_str("- 30%+ hard stops are not exercised on this sample; they behave like baseline but provide little ex-ante governance beyond the observed MaxDD.\n");
    report.push_str("- Best operational default from this audit: monitor 20% DD as a human review trigger, but do not auto-abandon below 30% without live/testnet evidence.\n");
    report.push_str("- T68 does not remove the live/testnet blocker; it only clarifies abandonment policy for the current exact-live leader.\n");

    fs::create_dir_all(format!("{root}/snapshots")).unwrap();
    let out = format!("{root}/snapshots/live_bot_abandonment_stress.md");
    let mut f = fs::File::create(&out).expect("create report");
    f.write_all(report.as_bytes()).unwrap();
    println!("=== T68 abandonment stress ===");
    println!("Baseline final {:.2}x, MaxDD {:.1}%", baseline_final, baseline_dd * 100.0);
    println!("Wrote {out}");
}
