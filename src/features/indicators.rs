use anyhow::Result;
use polars::prelude::*;

pub struct FeatureEngine;

impl FeatureEngine {
    /// Adds standard technicals + Relational metrics against a Benchmark (e.g., BTC)
    pub fn add_technicals(df: &DataFrame, benchmark_df: Option<&DataFrame>) -> Result<DataFrame> {
        let ewm_fast = EWMOptions {
            alpha: 1.0 / 12.0,
            adjust: true,
            bias: false,
            min_periods: 12,
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
            alpha: 1.0 / 9.0,
            adjust: true,
            bias: false,
            min_periods: 9,
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
