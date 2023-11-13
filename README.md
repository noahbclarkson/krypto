# Krypto

> Advanced Crypto-Futures Trading Algorithm

## Overview

Krypto is an innovative quantitative trading algorithm specifically designed for crypto-futures markets. It leverages the unique mass-correlation-relation algorithm to predict market movements and optimize returns. Krypto is equipped with functionalities for backtesting, live testing, and adjustable parameters for strategic trading.

## Features

- **Mass-Correlation-Relation Algorithm**: Utilizes complex relationships between various market indicators to predict price movements.
- **Comprehensive Data Analysis**: Integrates multiple technical indicators and normalizes data for accurate predictions.
- **Backtesting & Live Testing**: Evaluate the performance of trading strategies against historical data and in real-time market conditions.
- **Customizable Strategies**: Offers configurable parameters to tailor the algorithm according to specific trading preferences and market scenarios.

## Getting Started

### Prerequisites

- Rust (latest stable version)
- Cargo (Rust's package manager)
- Basic understanding of cryptocurrency markets and futures trading

### Installation

1. Clone the repository:
   ```bash
   git clone https://github.com/your-username/krypto.git
   ```
2. Navigate to the project directory:
    ```bash
    cd krypto
    ```
3. Run the project using Cargo:
    ```bash
    cargo run --release
    ```

## Configuration

Modify the config.yml file to set your trading preferences, such as target tickers, trading intervals, and backtesting parameters.

## What is the mass-correlation-relation algorithm?

The mass-correlation-relation algorithm is a self-developed predictive-model that uses the human-unobservable relations between technicals and other data in an array of tickers to make future predictions about the price change in another ticker. This is how it works:

### Data collection, computation and normalization

The algorithm is adjusted based on past data and so we collect historical data for the tickers in the config. For each ticker we get the past `periods` data points for the defined `interval`. By default we get 2,000 periods at 15m intervals for `BTCBUSD` and `ETHBUSD`.

This data contains open, high, low, close, volume and other data. However, we will transform this data into a number of technical indicators that will be used to make predictions and "train" the algorithm. For each ticker, for each period, we currently compute 6 technical "indicators" and store them in the `Candlestick` struct along with the open, close, high, low, volume, percentage change, and close time:

```rust
pub const TECHNICAL_COUNT: usize = 6;

#[derive(Debug, Getters, MutGetters, Setters)]
#[getset(get = "pub")]
pub struct Candlestick {
    open: f32,
    close: f32,
    high: f32,
    low: f32,
    volume: f32,
    #[getset(set = "pub")]
    p_change: f32,
    close_time: i64,
    #[getset(get = "pub", get_mut = "pub")]
    technicals: Box<[f32; TECHNICAL_COUNT]>,
}
```

We store the technicals in an array of floats that can be indexed using the `TechnicalType` enum:

```rust
#[derive(Debug, PartialEq)]
pub enum TechnicalType {
    PercentageChange,
    CandlestickRatio,
    StochasticOscillator,
    RelativeStrengthIndex,
    CommodityChannelIndex,
    VolumeChange,
}
```

The technicals are computed and then normalized to their [t-statistic](https://en.wikipedia.org/wiki/T-statistic) using the formula below:

$`t = \frac{{\bar{x} - \mu}}{{\frac{{s}}{{\sqrt{n}}}}}`$

In krypto the ```algorithm::normalize``` function performs this:

```rust
fn normalize(
    mut candles: Box<[TickerData]>,
    means: [f32; TECHNICAL_COUNT],
    stddevs: [f32; TECHNICAL_COUNT],
) -> Box<[TickerData]> {
    for ticker in candles.iter_mut() {
        for candle in ticker.candles_mut().iter_mut() {
            for (index, technical) in candle.technicals_mut().iter_mut().enumerate() {
                *technical = (*technical - means[index]) / stddevs[index];
                if technical.is_nan() || technical.is_infinite() {
                    *technical = 0.0;
                }
            }
        }
    }
    candles
}
```

### Relationships

Once we have calculated the normalized technicals for all the tickers at each data point we can start computing relationships. Given a target ticker $`T`$ and another ticker $`C`$, for every time period $`t`$, we compute the technical indicator for $`C`$ at $`t-d`$ (denoted as $`c_{t-d}`$) and the percentage change for $`T`$ at $`t`$ (denoted as $`pc(T_t)`$) for varying depths $`1..d`$. We then compute the product of these values for each depth and apply the hyperbolic tangent function to normalize the output. This can be represented mathematically as:

$`
R_{t}(T, C) = \tanh(c_{t-(1..d)} \cdot pc(T_t))
`$

where $`R_{t}(T, C)`$ is the computed relationship at time $`t`$ between the tickers $`T`$ and $`C`$, $`\tanh`$ is the hyperbolic tangent function, $`1..d`$ is all depths from 1 to d, and $`\cdot`$ represents multiplication.

This process is done for all possible pairs of $`T`$ and $`C`$, effectively mapping out the relationships between the technicals at varying depths for each ticker and the subsequent price changes of the target ticker.

We then average the relationships for each technical for each ticker for each depth to get an array of values of the size of the number of technicals multipled by the number of tickers multiplied by the depth.

This is performed by the `algorithm::compute_relationships` function below:

```rust
pub async fn compute_relationships(candles: &[TickerData], config: &Config) -> Box<[Relationship]> {
    let mut relationships = Vec::new();
    for (target_index, target_candles) in candles.iter().enumerate() {
        let tasks = candles
            .iter()
            .enumerate()
            .map(|(predict_index, predict_candles)| {
                compute_relationship(
                    target_index,
                    predict_index,
                    &target_candles,
                    predict_candles,
                    *config.depth(),
                )
            });
        futures::future::join_all(tasks)
            .await
            .into_iter()
            .for_each(|mut new_relationships| relationships.append(&mut new_relationships));
    }
    Box::from(relationships)
}

async fn compute_relationship(
    target_index: usize,
    predict_index: usize,
    target_candles: &TickerData,
    predict_candles: &TickerData,
    depth: usize,
) -> Vec<Relationship> {
    let mut results = vec![Vec::new(); TECHNICAL_COUNT * depth];
    for i in depth + 1..predict_candles.candles().len() - 1 {
        let target = &target_candles.candles()[i + 1].p_change().clone();
        for d in 0..depth {
            for (j, technical) in target_candles.candles()[i - d]
                .technicals()
                .iter()
                .enumerate()
            {
                results[d * TECHNICAL_COUNT + j].push((technical * target).tanh());
            }
        }
    }
    let correlations = results
        .iter()
        .map(|v| v.iter().sum::<f32>() / v.len() as f32)
        .collect::<Vec<f32>>();
    let mut relationships = Vec::new();
    for d in 0..depth {
        for j in 0..TECHNICAL_COUNT {
            let correlation = correlations[d * TECHNICAL_COUNT + j];
            relationships.push(Relationship {
                correlation,
                depth: d + 1,
                r_type: j,
                target_index,
                predict_index,
            });
        }

    }
    relationships
}
```

## Contributing

Contributions are what make the open-source community an amazing place to learn, inspire, and create. Any contributions you make are **greatly appreciated**.

1. Fork the Project
2. Create your Feature Branch (git checkout -b feature/AmazingFeature)
3. Commit your Changes (git commit -m 'Add some AmazingFeature')
4. Push to the Branch (git push origin feature/AmazingFeature)
5. Open a Pull Request

## License

Distributed under the MIT License. See LICENSE for more information


