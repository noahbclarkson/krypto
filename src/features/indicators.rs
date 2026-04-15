use anyhow::Result;
use polars::prelude::*;

pub struct FeatureEngine;

impl FeatureEngine {
    /// Adds standard technicals + Relational metrics against a Benchmark (e.g., BTC)
    pub fn add_technicals(df: &DataFrame, benchmark_df: Option<&DataFrame>) -> Result<DataFrame> {
        // NOTE: MACD fast EMA was 12 (Gerald Appel 1980s default). Hyperopt sweep
        // (examples/macd_fast_period_sweep.rs, 2026-04-08) found fast=28 dominates
        // on Sharpe (3.57 vs 1.13 baseline) and MaxDD (-49% vs -81%) in 9-universe
        // walk-forward validation. fast=28 is more robust: it is just above the slow
        // period (26), producing a tighter MACD that captures genuine trend shifts
        // without reacting to noise. This change propagates to all MACD-dependent
        // strategies (MACD+Regime, Turtle+MACD, ATR Breakout, head-to-head benchmark).
        let ewm_fast = EWMOptions {
            alpha: 1.0 / 28.0,
            adjust: true,
            bias: false,
            min_periods: 28,
            ignore_nulls: true,
        };
        let ewm_slow = EWMOptions {
            alpha: 1.0 / 26.0,
            adjust: true,
            bias: false,
            min_periods: 26,
            ignore_nulls: true,
        };
        let ewm_signal = EWMOptions {
            alpha: 1.0 / 10.0,
            adjust: true,
            bias: false,
            min_periods: 10,
            ignore_nulls: true,
        };
        let ewm_14 = EWMOptions {
            alpha: 1.0 / 14.0,
            adjust: true,
            bias: false,
            min_periods: 14,
            ignore_nulls: true,
        };

        let mut lazy = df
            .clone()
            .lazy()
            .with_columns(vec![
                col("close")
                    .ewm_mean(EWMOptions {
                        alpha: 1.0 / 20.0,
                        adjust: true,
                        bias: false,
                        min_periods: 20,
                        ignore_nulls: true,
                    })
                    .alias("ema_20"),
                col("close")
                    .ewm_mean(EWMOptions {
                        alpha: 1.0 / 50.0,
                        adjust: true,
                        bias: false,
                        min_periods: 50,
                        ignore_nulls: true,
                    })
                    .alias("ema_50"),
                col("close")
                    .ewm_mean(EWMOptions {
                        alpha: 1.0 / 200.0,
                        adjust: true,
                        bias: false,
                        min_periods: 200,
                        ignore_nulls: true,
                    })
                    .alias("ema_200"),
            ])
            .with_columns(vec![
                col("close").ewm_mean(ewm_fast).alias("ema_12"),
                col("close").ewm_mean(ewm_slow).alias("ema_26"),
            ])
            .with_column((col("ema_12") - col("ema_26")).alias("macd"))
            .with_column(col("macd").ewm_mean(ewm_signal).alias("macd_signal"))
            .with_column((col("macd") - col("macd_signal")).alias("macd_hist"))
            .with_columns(vec![
                (col("high") - col("low")).alias("tr1"),
                (col("high") - col("close").shift(lit(1)))
                    .abs()
                    .alias("tr2"),
                (col("low") - col("close").shift(lit(1))).abs().alias("tr3"),
            ])
            .with_column(
                when(
                    col("tr1")
                        .gt_eq(col("tr2"))
                        .and(col("tr1").gt_eq(col("tr3"))),
                )
                .then(col("tr1"))
                .when(col("tr2").gt_eq(col("tr3")))
                .then(col("tr2"))
                .otherwise(col("tr3"))
                .alias("tr"),
            )
            .with_column(col("tr").ewm_mean(ewm_14).alias("atr"))
            .with_column(col("close").diff(1, Default::default()).alias("diff"))
            .with_columns(vec![
                when(col("diff").gt(0.0))
                    .then(col("diff"))
                    .otherwise(lit(0.0))
                    .alias("gain"),
                when(col("diff").lt(0.0))
                    .then(col("diff").abs())
                    .otherwise(lit(0.0))
                    .alias("loss"),
            ])
            .with_columns(vec![
                col("gain").ewm_mean(ewm_14).alias("avg_gain"),
                col("loss").ewm_mean(ewm_14).alias("avg_loss"),
            ])
            .with_column(
                (lit(100.0) - (lit(100.0) / (lit(1.0) + (col("avg_gain") / col("avg_loss")))))
                    .alias("rsi"),
            );

        if let Some(bench) = benchmark_df {
            let bench_lazy = bench
                .clone()
                .lazy()
                .select([col("time"), col("close").alias("bench_close")]);

            lazy = lazy.join(
                bench_lazy,
                [col("time")],
                [col("time")],
                JoinArgs::new(JoinType::Left),
            );

            lazy = lazy
                .with_column((col("close") / col("bench_close")).alias("rs_ratio"))
                .with_column(col("bench_close").pct_change(lit(1)).alias("bench_ret"))
                .with_column(col("bench_ret").shift(lit(1)).alias("bench_ret_lag1"))
                .with_column(
                    col("rs_ratio")
                        .ewm_mean(EWMOptions {
                            alpha: 1.0 / 50.0,
                            adjust: true,
                            bias: false,
                            min_periods: 50,
                            ignore_nulls: true,
                        })
                        .alias("rs_ema_50"),
                );
        }

        Ok(lazy.collect()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_df() -> DataFrame {
        let closes: Vec<f64> = (0..100).map(|i| 100.0 + (i as f64 * 0.5)).collect();
        let highs: Vec<f64> = closes.iter().map(|c| c + 2.0).collect();
        let lows: Vec<f64> = closes.iter().map(|c| c - 2.0).collect();
        let volumes: Vec<f64> = vec![1000.0; 100];
        let times: Vec<i64> = (0..100).map(|i| 1700000000 + i * 3600).collect();

        df!(
            "time" => times,
            "close" => closes,
            "high" => highs,
            "low" => lows,
            "volume" => volumes,
        )
        .unwrap()
    }

    #[test]
    fn test_add_technicals_basic_columns() {
        let df = create_test_df();
        let result = FeatureEngine::add_technicals(&df, None).unwrap();

        // Check that new columns exist
        assert!(result.column("ema_20").is_ok(), "ema_20 should exist");
        assert!(result.column("ema_50").is_ok(), "ema_50 should exist");
        assert!(result.column("ema_200").is_ok(), "ema_200 should exist");
        assert!(result.column("macd").is_ok(), "macd should exist");
        assert!(
            result.column("macd_signal").is_ok(),
            "macd_signal should exist"
        );
        assert!(result.column("macd_hist").is_ok(), "macd_hist should exist");
        assert!(result.column("atr").is_ok(), "atr should exist");
        assert!(result.column("rsi").is_ok(), "rsi should exist");
    }

    #[test]
    fn test_add_technicals_with_benchmark() {
        let df = create_test_df();

        // Create benchmark (slightly different prices)
        let bench_closes: Vec<f64> = (0..100).map(|i| 50000.0 + (i as f64 * 10.0)).collect();
        let bench_highs: Vec<f64> = bench_closes.iter().map(|c| c + 100.0).collect();
        let bench_lows: Vec<f64> = bench_closes.iter().map(|c| c - 100.0).collect();
        let bench_volumes: Vec<f64> = vec![10000.0; 100];
        let times: Vec<i64> = (0..100).map(|i| 1700000000 + i * 3600).collect();

        let bench = df!(
            "time" => times,
            "close" => bench_closes,
            "high" => bench_highs,
            "low" => bench_lows,
            "volume" => bench_volumes,
        )
        .unwrap();

        let result = FeatureEngine::add_technicals(&df, Some(&bench)).unwrap();

        // Check benchmark-relative columns exist
        assert!(result.column("rs_ratio").is_ok(), "rs_ratio should exist");
        assert!(
            result.column("bench_close").is_ok(),
            "bench_close should exist"
        );
        assert!(result.column("bench_ret").is_ok(), "bench_ret should exist");
        assert!(result.column("rs_ema_50").is_ok(), "rs_ema_50 should exist");
    }

    #[test]
    fn test_rsi_range() {
        let df = create_test_df();
        let result = FeatureEngine::add_technicals(&df, None).unwrap();

        let rsi = result.column("rsi").unwrap().f64().unwrap();
        // RSI should be between 0 and 100 (allowing for NaN at start)
        for i in 50..100 {
            let val = rsi.get(i).unwrap_or(50.0);
            assert!(
                (0.0..=100.0).contains(&val),
                "RSI at {} should be in [0,100], got {}",
                i,
                val
            );
        }
    }

    #[test]
    fn test_macd_values() {
        let df = create_test_df();
        let result = FeatureEngine::add_technicals(&df, None).unwrap();

        let macd = result.column("macd").unwrap().f64().unwrap();
        let signal = result.column("macd_signal").unwrap().f64().unwrap();
        let hist = result.column("macd_hist").unwrap().f64().unwrap();

        // In an uptrend, MACD should eventually be positive
        let late_macd = macd.get(99).unwrap_or(0.0);
        assert!(
            late_macd > 0.0,
            "MACD should be positive in uptrend, got {}",
            late_macd
        );

        // Histogram = MACD - Signal
        for i in 50..100 {
            let m = macd.get(i).unwrap_or(0.0);
            let s = signal.get(i).unwrap_or(0.0);
            let h = hist.get(i).unwrap_or(0.0);
            if m.is_finite() && s.is_finite() && h.is_finite() {
                assert!(
                    (h - (m - s)).abs() < 0.001,
                    "MACD histogram should equal MACD - signal"
                );
            }
        }
    }

    #[test]
    fn test_atr_positive() {
        let df = create_test_df();
        let result = FeatureEngine::add_technicals(&df, None).unwrap();

        let atr = result.column("atr").unwrap().f64().unwrap();
        // ATR should always be positive (or NaN at start)
        for i in 30..100 {
            let val = atr.get(i).unwrap_or(0.0);
            assert!(val > 0.0, "ATR at {} should be positive, got {}", i, val);
        }
    }

    #[test]
    fn test_preserves_original_columns() {
        let df = create_test_df();
        let original_cols = df.get_column_names();
        let result = FeatureEngine::add_technicals(&df, None).unwrap();

        // All original columns should still exist
        for col in original_cols {
            assert!(
                result.column(col).is_ok(),
                "Original column {} should be preserved",
                col
            );
        }
    }
}
