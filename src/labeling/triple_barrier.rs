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

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_df(prices: &[f64], highs: &[f64], lows: &[f64]) -> DataFrame {
        let len = prices.len();
        df! [
            "close" => prices,
            "high" => highs,
            "low" => lows,
            "time" => (0..len as i64).collect::<Vec<_>>(),
        ]
        .unwrap()
    }

    #[test]
    fn test_hit_upper_barrier() {
        // Price starts at 100, volatility 0.01, pt_sl = (2.0, 2.0)
        // Upper barrier = 100 * (1 + 0.01 * 2.0) = 102
        // High at bar 2 = 103 > 102, should hit upper
        let df = create_test_df(
            &[100.0, 101.0, 102.0, 103.0],
            &[100.0, 101.5, 103.0, 103.5],
            &[99.5, 100.5, 101.5, 102.5],
        );
        let vol = Series::new("vol", vec![0.01; 4]);

        let labels = apply_triple_barrier(&df, &vol, 5, (2.0, 2.0)).unwrap();

        assert_eq!(labels[0], 1, "Should hit upper barrier");
    }

    #[test]
    fn test_hit_lower_barrier() {
        // Price starts at 100, volatility 0.01, pt_sl = (2.0, 2.0)
        // Lower barrier = 100 * (1 - 0.01 * 2.0) = 98
        // Low at bar 2 = 97 < 98, should hit lower
        let df = create_test_df(
            &[100.0, 99.0, 98.0, 97.0],
            &[100.5, 99.5, 98.5, 97.5],
            &[99.5, 98.5, 97.0, 96.5],
        );
        let vol = Series::new("vol", vec![0.01; 4]);

        let labels = apply_triple_barrier(&df, &vol, 5, (2.0, 2.0)).unwrap();

        assert_eq!(labels[0], -1, "Should hit lower barrier");
    }

    #[test]
    fn test_timeout() {
        // Price stays within barriers
        let df = create_test_df(
            &[100.0, 100.5, 101.0, 101.5],
            &[100.5, 101.0, 101.5, 102.0],
            &[99.5, 100.0, 100.5, 101.0],
        );
        let vol = Series::new("vol", vec![0.01; 4]);

        let labels = apply_triple_barrier(&df, &vol, 3, (2.0, 2.0)).unwrap();

        // With volatility 0.01 and pt_sl=2.0, barriers are at 102/98
        // Prices stay within range, should timeout
        assert_eq!(labels[0], 0, "Should timeout");
    }

    #[test]
    fn test_short_horizon() {
        // Horizon of 1 means no bars to check, always timeout
        let df = create_test_df(
            &[100.0, 110.0, 90.0],
            &[100.5, 111.0, 91.0],
            &[99.5, 109.0, 89.0],
        );
        let vol = Series::new("vol", vec![0.01; 3]);

        let labels = apply_triple_barrier(&df, &vol, 1, (2.0, 2.0)).unwrap();

        assert_eq!(labels[0], 0, "Should timeout with horizon=1");
    }

    #[test]
    fn test_asymmetric_barriers() {
        // pt_sl = (3.0, 1.0) - wider upper, tighter lower
        // Upper = 100 * 1.03 = 103
        // Lower = 100 * 0.99 = 99
        let df = create_test_df(
            &[100.0, 99.5, 99.0, 98.5],
            &[100.5, 100.0, 99.5, 99.0],
            &[99.5, 99.0, 98.5, 98.0],
        );
        let vol = Series::new("vol", vec![0.01; 4]);

        let labels = apply_triple_barrier(&df, &vol, 5, (3.0, 1.0)).unwrap();

        assert_eq!(labels[0], -1, "Should hit tighter lower barrier");
    }

    #[test]
    fn test_high_volatility() {
        // High volatility = wider barriers
        // With vol=0.05 and pt_sl=(2.0, 2.0):
        // Upper = 100 * 1.10 = 110
        // Lower = 100 * 0.90 = 90
        let df = create_test_df(
            &[100.0, 105.0, 108.0, 109.0],
            &[102.0, 107.0, 109.5, 110.0],
            &[98.0, 103.0, 106.0, 107.0],
        );
        let vol = Series::new("vol", vec![0.05; 4]);

        let labels = apply_triple_barrier(&df, &vol, 5, (2.0, 2.0)).unwrap();

        // Should timeout - doesn't hit 110 or 90
        assert_eq!(labels[0], 0, "Should timeout with wide barriers");
    }
}
