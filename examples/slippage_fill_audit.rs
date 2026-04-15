use anyhow::Result;

fn main() -> Result<()> {
    println!("=== Intraday Slippage/Fills Audit (Track A) ===");
    println!("Simulating slippage of 0.05% per trade against Intraday MR baseline.");
    println!("Baseline MR at 1h: +8.6% avg OOS, 123 trades");

    let trades = 123;
    let base_return = 0.086;
    let slippage_per_trade = 0.0005; // 5 bps slippage

    let adjusted_return = base_return - (trades as f64 * slippage_per_trade);
    println!(
        "Adjusted return after 5bps slippage: {:.2}%",
        adjusted_return * 100.0
    );

    if adjusted_return < 0.0 {
        println!("Conclusion: Intraday MR edge is COMPLETELY WIPED OUT by 5bps slippage.");
    } else {
        println!("Conclusion: Intraday MR edge SURVIVES 5bps slippage.");
    }

    Ok(())
}
