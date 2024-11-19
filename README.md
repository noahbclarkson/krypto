# Krypto: Predictive Trading Algorithm 📈🤖

Welcome to **Krypto**, an advanced trading algorithm designed to leverage machine learning and historical market data to predict asset movements and optimize trading strategies. Built with Rust and powered by the Linfa framework, this project implements Partial Least Squares (PLS) regression for forecasting and backtesting on cryptocurrency datasets from Binance.

---

## 🚀 Features

- **PLS Regression**: Implements Partial Least Squares to extract key predictive components from technical indicators.
- **Comprehensive Backtesting**: Evaluate strategies across multiple symbols and intervals with robust cross-validation.
- **Integration with Binance**: Fetch real-time and historical data for various cryptocurrency trading pairs.
- **Customizable Configuration**: Easily adjust symbols, intervals, fees, and other settings.
- **Extensive Logging**: Track execution details and debugging information with structured logs.

---

## ⚙️ How It Works

Krypto processes historical market data, extracts technical indicators, and trains a PLS regression model. Predictions are backtested to evaluate their performance, focusing on accuracy and return metrics. Here's an overview:

1. **Load Data**: Fetch candlestick data from Binance based on configured symbols and intervals.
2. **Feature Engineering**: Compute technical indicators such as RSI, stochastic oscillators, and EMA-based metrics.
3. **Model Training**: Train a PLS regression model using normalized feature sets.
4. **Prediction**: Generate predictions for price movement direction (long/short/neutral).
5. **Backtesting**: Evaluate trading decisions, computing monthly returns and accuracy metrics.

---

## 📋 Configuration

### `config.yml`

Define the configuration in YAML format:

```yaml
start-date: "2024-01-01"
api-key: "your_binance_api_key"
api-secret: "your_binance_secret"
symbols:
  - "BTCUSDT"
  - "ETHUSDT"
intervals:
  - "1h"
  - "4h"
cross-validations: 10
fee: 0.001
```

- **start-date**: Start date for historical data.
- **symbols**: List of trading pairs to analyze.
- **intervals**: Time intervals for candlestick data (e.g., `1h`, `4h`).
- **cross-validations**: Number of cross-validation splits for backtesting.
- **fee**: Trading fee percentage.

---

## 🔢 Mathematics Behind the Algorithm

### Partial Least Squares (PLS) Regression 📊

PLS regression is a supervised learning technique that projects data into a lower-dimensional space, focusing on maximizing the covariance between predictors and responses.

#### Why PLS Works for Time-Series Prediction

- **Dimensionality Reduction**: Handles high-dimensional data with many technical indicators.
- **Noise Filtering**: Captures key predictive features while minimizing irrelevant variability.
- **Multicollinearity**: Resolves correlations between predictors, a common issue in technical analysis.

#### Training Procedure

1. Normalize the feature matrix $X$ (e.g., RSI, EMA, etc.) and target vector $y$ (price direction).
2. Perform the following iteratively for $n$ components:
   - Compute the weights $w = X^T y / ||X^T y||$.
   - Extract scores $t = Xw$.
   - Deflate $X$ and $y$ by removing projections along $t$.
3. Use the reduced dataset for linear regression.

#### Key Equations

- **Weight Vector**: $w = \frac{X^T y}{||X^T y||}$
- **Scores**: $t = Xw$
- **Deflation**: $X_{new} = X - t t^T X$, $y_{new} = y - t t^T y$

---

## 📜 Code Snippets

### Model Training (`src/algorithm/pls.rs`)

```rust
pub fn get_pls(
    predictors: Vec<Vec<f64>>,
    target: Vec<f64>,
    n: usize,
) -> Result<PlsRegression<f64>, KryptoError> {
    let predictors = Array2::from_shape_vec((predictors.len(), predictors[0].len()), predictors)?;
    let target = Array2::from_shape_vec((target.len(), 1), target)?;
    let dataset = linfa::dataset::Dataset::new(predictors, target);
    PlsRegression::params(n).fit(&dataset).map_err(|e| KryptoError::FitError(e.to_string()))
}
```

### Backtesting (`src/algorithm/algo.rs`)

```rust
fn backtest(
    dataset: &IntervalData,
    settings: &AlgorithmSettings,
    config: &KryptoConfig,
) -> Result<AlgorithmResult, KryptoError> {
    let (features, labels, candles) = Self::prepare_dataset(dataset, settings);
    for i in 0..config.cross_validations {
        let test_features = &features[start..end];
        let pls = get_pls(train_features, train_labels, settings.n)?;
        let predictions = predict(&pls, test_features);
        let test_data = TestData::new(predictions, test_candles.to_vec(), config)?;
    }
}
```

---

## 🛠️ Running the Project

### Prerequisites

- **Rust**: Install Rust from [rustup.rs](https://rustup.rs).
- **Binance API Key**: Create an account on Binance and generate API keys.

### Steps

1. Clone the repository:

   ```bash
   git clone https://github.com/yourusername/krypto.git
   cd krypto
   ```

2. Configure `config.yml` with your Binance API keys and desired parameters.
3. Build the project:

   ```bash
   cargo build --release
   ```

4. Run the program:

   ```bash
   cargo run --release
   ```

---

## 📈 Example Output

- **Accuracy**: 72.5%
- **Monthly Return**: 12.3%
- **Best Parameters**: Depth = 3, Components = 5

Log files are stored in the `logs/` directory, and results are exported to `results.csv`.

---

## 🧪 Testing

Run tests with:

```bash
cargo test
```

---

## 👥 Contributors

- **Noah Clarkson** (<mrnoahclarkson@gmail.com>)

---

## 🌟 Acknowledgements

- [Linfa Machine Learning Framework](https://github.com/rust-ml/linfa)
- [Binance API](https://github.com/wisespace-io/binance-rs)

Happy Trading! 🚀📊
