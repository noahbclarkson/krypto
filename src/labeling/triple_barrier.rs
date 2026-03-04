#![allow(clippy::needless_range_loop)]
use polars::prelude::*;

#[derive(Debug, Clone, Copy)]
pub enum BarrierEvent {
    HitUpper = 1,
    HitLower = -1,
    TimeOut = 0,
}

/// Generates labels based on:
/// 1. Upper Barrier (Profit Take)
/// 2. Lower Barrier (Stop Loss)
/// 3. Vertical Barrier (Time Expiry)
pub fn apply_triple_barrier(
    df: &DataFrame,
    volatility: &Series,
    t_horizon: usize,
    pt_sl: (f64, f64),
) -> Result<Vec<i8>, PolarsError> {
    let closes = df.column("close")?.f64()?;
    let highs = df.column("high")?.f64()?;
    let lows = df.column("low")?.f64()?;
    let vols = volatility.f64()?;

    let len = closes.len();
    let mut labels = vec![0i8; len];

    for i in 0..len {
        let start_price = closes.get(i).unwrap_or(0.0);
        let vol = vols.get(i).unwrap_or(0.01);

        let upper = start_price * (1.0 + vol * pt_sl.0);
        let lower = start_price * (1.0 - vol * pt_sl.1);

        let end_idx = std::cmp::min(i + t_horizon, len);
        let mut outcome = BarrierEvent::TimeOut;

        for j in (i + 1)..end_idx {
            let h = highs.get(j).unwrap_or(0.0);
            let l = lows.get(j).unwrap_or(0.0);

            if l <= lower {
                outcome = BarrierEvent::HitLower;
                break;
            }
            if h >= upper {
                outcome = BarrierEvent::HitUpper;
                break;
            }
        }
        labels[i] = outcome as i8;
    }

    Ok(labels)
}
