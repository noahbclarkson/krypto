# krypto

----------------------

## What is Krypto?

Krypto is a quantitative crypto-futures trading algorithm based on the self-developed mass-correlation-relation algorithm. Krypto comes fully equppied with backtesting, livetesting, and a number of configurable parameters to optimize cash return.

## What is the mass-correlation-relation algorithm?

The mass-correlation-relation algorithm is a self-developed predictive-model that uses the human-unobservable relations between technicals and other data in an array of tickers to make future predictions about the price change in another ticker. This is how it works:

Firt we get an array of tickers and get their historical data for a large time frame at a certain interval. Krypto's default is 7,000 data points at 15 minute intervals for the 18 possible tickers on binance-futures (BUSD). Then, for each interval, for each ticker, we compute a number of technical indicators. Most of these indicators are well-known technical indicators that have been used in finance for decades. These include; the Stochastic Oscillator, the Relative Strength Index, and the Commodity Channel Index. We also compute a number of custom indicators. These are explained below:

### Indicators

- #### Percentage Change

This is the percentage change from the close price at period t-1 to the close price at period t:

$`PC(x_t)= \frac{x_t - x_{t-1}}{x_{t-1}} \cdot 100`$

In krypto it is computed using the `math::change` function:

```rust
// Calculates and returns the percentage change between two values.
// start: Initial value
// end: Final value
pub fn change(start: f64, end: f64) -> f64 {
    if start == 0.0 {
        return 0.0; // Avoid division by zero
    }
    (end - start) / start * 100.0
}
```

- #### Candlestick Ratio

This is the ratio of the difference of the top wick and bottom wick to the body of the candlestick. The tanh function is used to normalize the output:

Let's denote:
$`c_{\text{high}}`$ as the highest price in the candle
$`c_{\text{low}}`$ as the lowest price in the candle
$`c_{\text{open}}`$ as the opening price of the candle
$`c_{\text{close}}`$ as the closing price of the candle

The body of the candlestick is given by $`body = \max(c_{\text{close}}, c_{\text{open}}) - \min(c_{\text{close}}, c_{\text{open}})$, the top wick is $top_{\text{wick}} = c_{\text{high}} - \max(c_{\text{close}}, c_{\text{open}})$, and the bottom wick is $bottom_{\text{wick}} = \min(c_{\text{close}}, c_{\text{open}}) - c_{\text{low}}`$.

The sum of the differences of the top wick and bottom wick is given by $`wick_{\text{sum}} = top_{\text{wick}} - bottom_{\text{wick}}`$.

Then the candlestick ratio (CR ratio) is given by:
$`
CR(c) = \begin{cases} \text{sgn}(wick_{\text{sum}}) & \text{if } |body| < \epsilon \\ \tanh\left(\frac{wick_{\text{sum}}}{body}\right) & \text{otherwise} \end{cases}
`$

Here, $`\epsilon`$ is a very small constant (close to zero) to avoid division by zero, $`\tanh`$ is the hyperbolic tangent function, and $`\text{sgn}`$ is the function that extracts the sign of a real number.

In krypto it is computed using the `math::cr_ratio` function:

```rust
// Computes the CR ratio of a candlestick.
// The CR ratio is the ratio of the difference between the top and bottom wick to the body
// The tanh function is used to normalize the output.
// candle: A reference to the Candlestick struct
pub fn cr_ratio(candle: &Candlestick) -> f64 {
    let max_body = candle.close.max(candle.open);
    let min_body = candle.close.min(candle.open);
    let body = max_body - min_body;
    let top_wick = candle.high - max_body;
    let bottom_wick = min_body - candle.low;
    let wick_sum = top_wick - bottom_wick;

    if body.abs() < f64::EPSILON {
        return wick_sum.signum();
    } else {
        return (wick_sum / body).tanh();
    }
}
```

- #### Volume Change

The volume change indicator is computed in the same manner as the percentage change indicator (using `math::change`), but instead of using the close price, we use the volume.

### Relationships

Given a target ticker $`T`$ and another ticker $`C`$, for every time period $`t`$, we compute the technical indicator for $`C`$ at $`t-1`$ (denoted as $`c_{t-1}`$) and the percentage change for $`T`$ at $`t`$ (denoted as $`pc(T_t)`$). We then compute the product of these two values and apply the hyperbolic tangent function to normalize the output. This can be represented mathematically as:

$`
R_{t}(T, C) = \tanh(c_{t-1} \cdot pc(T_t))
`$

where $`R_{t}(T, C)`$ is the computed relationship at time $`t`$ between the tickers $`T`$ and $`C`$, $`\tanh`$ is the hyperbolic tangent function, and $`\cdot`$ represents multiplication.

This process is done for all possible pairs of $`T`$ and $`C`$, effectively mapping out the relationships between the technicals of each ticker and the price changes of the target ticker.

We then average the relationships for each technical for each ticker to get an array of values of the size of the number of technicals multipled by the number of tickers.

This is performed in the `Algorithm::calculate_relationships` function:

```rust
    fn calculate_relationship(
        &mut self,
        ticker: &String,
        candlesticks: &[Candlestick],
        other_ticker_data: TickerData,
    ) {
        // Loop throuh enum and create a Vec for each relationship type
        let mut results = Vec::new();
        for _ in RelationshipType::iter() {
            results.push(Vec::new());
        }
        let other_ticker = &other_ticker_data.ticker;
        let other_candlesticks = &other_ticker_data.candlesticks;
        for i in 1..candlesticks.len() {
            let candlestick = &candlesticks[i];
            let other_candlestick = &other_candlesticks[i - 1];
            let target = candlestick.pc;
            for (i, r_type) in RelationshipType::iter().enumerate() {
                let n = other_candlestick.get_technical(&r_type);
                results[i].push((target * n).tanh());
            }
        }
        let correlations = results
            .iter()
            .map(|x| x.iter().sum::<f64>() / x.len() as f64)
            .collect::<Vec<_>>();
        for (i, r_type) in RelationshipType::iter().enumerate() {
            let correlation = correlations[i];
            let relationship = Relationship {
                predictor: other_ticker.clone(),
                target: ticker.clone(),
                correlation,
                relationship_type: r_type,
                weight: 1.0,
            };
            self.relationships.push(relationship);
        }
    }
```

These relationships are stored in the `Algorithm::relationships` vector of `Relationship` structs:

```rust
#[derive(Debug, Clone)]
pub struct Relationship {
    // The ticker that is being used to make the prediction
    predictor: String,
    // The ticker that is being predicted
    target: String,
    // The correlation between the predictor and the target
    correlation: f64,
    // The type of relationship
    relationship_type: RelationshipType,
    // The weight of the relationship
    weight: f64,
}
```

### Normalization

Once we have collected and computed the technicals for every ticker. We normalize the data around the percentage change. In summary, we want all our technicals to have the same standard deviation and a mean of 0 (the theoretical mean of the percentage change). This is performed in the `HistoricalData::normalize_technicals` function below:

```rust
pub fn normalize_technicals(&mut self, means: &[f64], stds: &[f64]) {
        for (i, r_type) in RelationshipType::iter().enumerate() {
            if r_type == RelationshipType::PercentageChange {
                continue;
            }
            for ticker_data in &mut self.data {
                for candle in &mut ticker_data.candlesticks {
                    candle.set_techincal(
                        &r_type,
                        (candle.get_technical(&r_type) - means[i]) / stds[i] * stds[0],
                    );
                }
            }
        }
    }
```

### Predictions

Once we have the array of relationship values we can make predictions about a future time period by getting the sum of the correlation and the actual technical value for each technical for each ticker. This can be represented mathematically as:

Given a future time period $t$, and for each ticker $C$, the predicted price change $\Delta P$ for the target ticker $T$ can be calculated by summing the products of the correlation of the technical value for ticker $C$ at time $t$ and its corresponding weight in the relationship, for all tickers and all technicals. This can be represented mathematically as:

$\Delta P_{T,t} = \sum_{C, \text{tech}} (corr_{C,\text{tech}} \cdot \text{tech}_{C,t})$

where:

$corr_{C,\text{tech}}$ is the correlation between the technical indicator of ticker $C$ and the price change of the target ticker $T$,
$\text{tech}_{C,t}$ is the technical indicator value for ticker $C$ at time $t$, and
$\cdot$ represents multiplication.
The sum $\sum$ goes over all tickers $C$ and all technical indicators.

It is performed using the `Algorithm::predict` function below:

```rust
    pub fn predict(&self, ticker: &str, target_pos: usize) -> f64 {
        let mut score = 0.0;
        let predict_pos = target_pos - 1;
        for ticker_data in &self.data.data {
            let other_candlesticks = &ticker_data.candlesticks;
            for relationship in &self.relationships {
                if relationship.target == ticker && relationship.predictor == ticker_data.ticker {
                    score += other_candlesticks[predict_pos]
                        .get_technical(&relationship.relationship_type)
                        * relationship.correlation
                        * relationship.weight;
                }
            }
        }
        score
    }
```

### Optimization

Now we have an algorithm that can make predictions about the future price change of a ticker based on the technicals of other tickers. But how do we know which technicals to use? And how do we know how much weight to give each relationship? This is where the optimization comes in.

We test the algorithm using `Algorithm::test` which returns a result in cash. It loops through all the time-periods in $`T`$ and makes a prediction for each. We start with `$1,000` and let the algorithm trade through all its time-periods.

Each `Relationship` struct holds a weight value. We randomly assign these weights and run tests on the algorithm. We then keep the weights that give the best result and randomly change the weights again. We repeat this process until we have a set of weights that give the best result.

Binance-futures also allows for leverage. We can use this to increase the amount of money we make. However, we can also use this to increase the amount of money we lose. We can optimize the leverage by looping through all possible leverage values and finding the one that gives the best result.

### Why does this work?

The mass-correlation-relation algorithm works because it is able to find relationships between technicals and price changes that are not observable by humans. It is able to find these relationships by using a large amount of data and a large number of technicals. It is also able to find relationships that are not linear. This is because the hyperbolic tangent function is used to normalize the output of the relationship between $`-1`$ and $`1`$. This allows the algorithm to find relationships that are not linear.

Additionally, part of the issue with using more complicated algorithms is mass overfitting. Because the data is close to random, it is very easy to overfit the data with a large amount of weights in something like a neural network. By averaging the correlation we minimize the possibility of overfitting and can increase the amount of technicals, weights or tickers without overfitting.

Furthermore, the algorithm is optimized to make the most money and NOT to be the most accurate. In fact, sometimes accuracy can be below 50% and the algorithm can still be making a significant return.

### Drawbacks

While the mass-correlation-relation algorithm is incredibly effective at making money, because of the accuracy drawback the algorithm makes a lot of trades that are incorrect. The lowest fee rate that is possible for binance-futures is 0.0270% (with BUSD and BNB in your account). We are essentially forced to make taker trades because we are trading on the 15 minute interval. Because this fee applies to both the buy and sell orders, we are essentially paying 0.0540% per trade. This means that we need to make at least 0.0540% on each trade to break even. This means that, overall, the algorithm is FAR less effective than it would be with no fees. In fact, with no fees the algorithm is predicted to make returns of over 10,000% in less than 3 months. However, with fees, the algorithm is predicted to make far less. This is why it is important to optimize the algorithm to make the most money and not to be the most accurate. In the test function we also account for fees by subtracting the fee from the cash amount but we do not account for slippage. However, we do account for the case that the algorithm makes a trade and the next trade is the same direction so fees are not applied.

## How to use Krypto

### Installation

To install krypto, first clone the repository:

```bash
git clone
```

Then, either build the project:

```bash
cargo build --release
```

Or run it directly using cargo:

```bash
cargo run --release
```

### Configuration

Krypto creates a config.yml and tickers.txt file in the current directory. The config.yml file contains all the configuration options for the algorithm. The tickers.txt file contains all the tickers that the algorithm will use. The tickers.txt file is created by default with all the tickers on binance-futures that use BUSD. You can change the tickers in the tickers.txt file to any tickers you want. The config.yml file is created by default with the following configuration:

```yaml
periods: 7000,
interval: "15m",
margin: 2.0,
fee: 0.054,
```

The periods option is the number of periods that the algorithm will use to make predictions. The interval option is the interval that the algorithm will use to get the data. The margin option is the default margin that the algorithm will use to trade. The fee option is the fee that the algorithm will use to calculate the fees for each trade.

### Running the algorithm

The algorithm will run asynchronously and collect all of the data from the binance API. It will then perform all of the necessary calculations and optimizations. This process can take a long time depending on the number of tickers and the number of periods. Once the algorithm is done, it will run a live test which uses realtime data to make predictions.
