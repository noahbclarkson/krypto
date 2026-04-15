//! Exchange reserve / balance lane opener.
//!
//! Purpose:
//! - test whether the project can honestly access exchange reserve history now
//! - avoid another hand-wavy "reserve / whale-flow next" promise without probing the data path
//! - if blocked, surface the blocker concretely and stop

use anyhow::Result;
use chrono::{TimeZone, Utc};
use krypto::data::ExchangeBalanceLoader;
use polars::prelude::DataType;

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== EXCHANGE BALANCE / RESERVE DEPTH AUDIT ===\n");
    println!("Goal: determine whether the exchange reserve lane is actually accessible enough to open honest research.\n");

    let loader = match ExchangeBalanceLoader::from_env() {
        Ok(loader) => loader,
        Err(err) => {
            println!("Status: BLOCKED");
            println!("Reason: {err}");
            println!(
                "Honest read: the reserve / whale-flow lane is still infra-blocked right now."
            );
            println!("Best next move: either provide COINGLASS_API_KEY or treat this as a data-access blocker and avoid fake backtests.\n");
            return Ok(());
        }
    };

    let df = match loader.fetch_total_balance("BTC", false).await {
        Ok(df) => df,
        Err(err) => {
            println!("Status: BLOCKED");
            println!("Reason: {err}");
            println!("Honest read: the reserve / whale-flow lane is conceptually promising but currently inaccessible from this environment.");
            return Ok(());
        }
    };

    let rows = df.height();
    let times = df.column("time")?.cast(&DataType::Int64)?;
    let ts: Vec<i64> = times.i64()?.into_iter().flatten().collect();

    if ts.len() < 2 {
        println!("Status: TOO SHALLOW");
        println!("Rows returned: {rows}");
        println!("Honest read: reserve data access exists, but depth is not yet credible.");
        return Ok(());
    }

    let first = *ts.first().unwrap();
    let last = *ts.last().unwrap();
    let span_days = (last - first) as f64 / 86_400_000.0;
    let first_dt = Utc.timestamp_millis_opt(first).single().unwrap();
    let last_dt = Utc.timestamp_millis_opt(last).single().unwrap();

    let latest_balance = df
        .column("total_exchange_balance")?
        .f64()?
        .get(rows - 1)
        .unwrap_or(0.0);
    let latest_change_7 = df
        .column("reserve_change_7")?
        .f64()?
        .get(rows - 1)
        .unwrap_or(0.0);

    println!("Status: LIVE");
    println!("Rows: {rows}");
    println!("Span days: {:.1}", span_days);
    println!("Window: {} -> {}", first_dt, last_dt);
    println!("Latest total exchange balance: {:.3}", latest_balance);
    println!(
        "Latest 7-step reserve change: {:.3}%",
        latest_change_7 * 100.0
    );

    let honest = if rows >= 365 {
        "usable for first daily reserve-state research"
    } else if rows >= 90 {
        "limited but exploratory"
    } else {
        "too shallow"
    };

    println!("Honest read: {honest}");
    println!(
        "Next: if usable, build a reserve-state annotation benchmark before any strategy claims."
    );

    Ok(())
}
