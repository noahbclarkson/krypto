use std::fmt;

use binance::rest_model::OrderSide;
use chrono::{DateTime, Utc}; // Added DateTime, Utc
use serde::Serialize; // Added Serialize for TradeLogEntry
use tracing::{debug, warn};

use crate::{
    config::KryptoConfig,
    data::candlestick::Candlestick,
    error::KryptoError,
    util::{date_utils::days_between, math_utils::std_deviation}, // Added median
};

const STARTING_CASH: f64 = 1000.0;
const RISK_FREE_RATE: f64 = 0.0; // Used for Sharpe Ratio calculation

// --- Trade Log Entry ---
#[derive(Debug, Clone, Serialize)]
pub struct TradeLogEntry {
    pub timestamp: DateTime<Utc>, // Time the trade was closed/evaluated
    pub symbol: String,
    #[serde(with = "order_side_serializer")] // Custom serializer for OrderSide
    pub side: OrderSide, // Side of the position being closed
    pub entry_price: f64,
    pub exit_price: f64,
    pub quantity: f64, // Quantity traded (absolute value)
    pub pnl: f64,      // Profit or Loss for this trade (net of fees)
    pub pnl_pct: f64,  // PnL as a percentage of entry value (approx)
    pub fee: f64,      // Fee paid for this trade
    pub cash_after_trade: f64,
    pub equity_after_trade: f64, // Total portfolio value after trade
    pub reason: String,          // e.g., "Signal Flip", "Stop Loss", "Take Profit", "End of Test"
}

// Custom serializer for OrderSide to output "BUY" or "SELL"
mod order_side_serializer {
    use binance::rest_model::OrderSide;
    use serde::Serializer;

    pub fn serialize<S>(side: &OrderSide, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match side {
            OrderSide::Buy => serializer.serialize_str("BUY"),
            OrderSide::Sell => serializer.serialize_str("SELL"),
        }
    }
}

// --- Simulation Output ---
#[derive(Debug, Clone)]
pub struct SimulationOutput {
    pub metrics: TestData, // The original metrics struct
    pub trade_log: Vec<TradeLogEntry>,
    pub equity_curve: Vec<(DateTime<Utc>, f64)>,
}

// --- Test Data (Metrics Summary) ---
#[derive(Debug, Clone, PartialEq)]
pub struct TestData {
    pub accuracy: f64,
    pub monthly_return: f64,
    pub final_cash: f64,
    pub sharpe_ratio: f64,
    pub max_drawdown: f64,
    pub total_trades: u32,
    pub win_rate: f64,
}

impl TestData {
    /// Runs the trading simulation and returns detailed output including metrics, trade log, and equity curve.
    ///
    /// # Arguments
    /// * `symbol` - The symbol being traded.
    /// * `predictions` - Raw model output (e.g., predicted value or probability).
    /// * `candles` - Corresponding candlesticks for simulation.
    /// * `config` - Trading configuration (fees, margin, stop-loss, take-profit).
    ///
    /// # Returns
    /// A Result containing the `SimulationOutput` or a `KryptoError`.
    pub fn run_simulation(
        symbol: &str, // Add symbol parameter
        predictions: &[f64],
        candles: &[Candlestick],
        config: &KryptoConfig,
    ) -> Result<SimulationOutput, KryptoError> {
        if candles.is_empty() || predictions.is_empty() {
            return Err(KryptoError::EmptyCandlesOrPredictions);
        }
        if candles.len() != predictions.len() {
            return Err(KryptoError::UnequalCandlesAndPredictions(
                candles.len(),
                predictions.len(),
            ));
        }

        let fee = config.fee.unwrap_or(0.0);
        let margin = config.margin;
        let stop_loss_pct = config.trade_stop_loss_percentage;
        let take_profit_pct = config.trade_take_profit_percentage;

        let start_time = candles.first().unwrap().open_time; // Get start time
        let mut inner = InnerTestData::new(STARTING_CASH, symbol.to_string(), start_time); // Pass symbol and time
        let mut position: Option<Position> = None;

        for (prediction, candle) in predictions.iter().zip(candles.iter()) {
            let prediction_signal = prediction.signum(); // Use signum for direction (-1, 0, 1)
            let current_price = candle.close;
            let current_time = candle.close_time; // Get current time
            

            let mut closed_by_sl_tp = false;
            let mut close_reason: String; // Declare here, assign before use

            // --- Stop-Loss / Take-Profit Check ---
            if let Some(ref mut pos) = position {
                let entry_price = pos.entry_price();
                let current_return = pos.get_return(current_price);

                // Check Stop Loss
                if let Some(sl) = stop_loss_pct {
                    if current_return <= -sl {
                        debug!(
                            "Stop Loss triggered at price {} (Entry: {}, Return: {:.2}%)",
                            current_price,
                            entry_price,
                            current_return * 100.0
                        );
                        close_reason = "Stop Loss".to_string(); // Assign reason
                        inner.close_position(
                            pos,
                            current_price,
                            current_time,
                            fee,
                            margin,
                            close_reason, // Use assigned reason
                        );
                        closed_by_sl_tp = true;
                    }
                }

                // Check Take Profit (only if not already closed by SL)
                if !closed_by_sl_tp {
                    if let Some(tp) = take_profit_pct {
                        if current_return >= tp {
                            debug!(
                                "Take Profit triggered at price {} (Entry: {}, Return: {:.2}%)",
                                current_price,
                                entry_price,
                                current_return * 100.0
                            );
                            close_reason = "Take Profit".to_string(); // Assign reason
                            inner.close_position(
                                pos,
                                current_price,
                                current_time,
                                fee,
                                margin,
                                close_reason, // Use assigned reason
                            );
                            closed_by_sl_tp = true;
                        }
                    }
                }

                if closed_by_sl_tp {
                    position = None; // Reset position after SL/TP closure
                } else {
                    // Update equity curve based on unrealized P/L if holding
                    inner.update_equity(pos, current_price, current_time, margin);
                }
            }

            // --- New Position / Signal Flip Logic ---
            // This logic runs regardless of whether SL/TP triggered, but acts only if position is None
            if position.is_none() {
                // If flat (or just closed by SL/TP)
                let desired_position = match prediction_signal {
                    s if s > 0.0 => Some(Position::Long(current_price)),
                    s if s < 0.0 => Some(Position::Short(current_price)),
                    _ => None, // Neutral signal -> stay flat
                };

                if let Some(new_pos) = desired_position {
                    debug!(
                        "Entering new position: {:?} at price {}",
                        new_pos, current_price
                    );
                    position = Some(new_pos);
                    // Don't update equity here, wait for next candle's update_equity or close_position
                } else {
                    // If flat and signal is neutral, update equity with current cash
                    inner.update_equity_value(inner.cash, current_time);
                }
            } else {
                // If holding a position not closed by SL/TP
                if let Some(ref current_pos) = position {
                    let desired_side = match prediction_signal {
                        s if s > 0.0 => OrderSide::Buy,
                        s if s < 0.0 => OrderSide::Sell,
                        _ => current_pos.side(), // Stay in current position side if neutral signal
                    };

                    // If prediction signal flips the side
                    if current_pos.side() != desired_side {
                        debug!(
                            "Signal flipped. Closing {:?} at {}, Entering {:?} at {}",
                            current_pos.side(),
                            current_price,
                            desired_side,
                            current_price
                        );
                        close_reason = "Signal Flip".to_string(); // Assign reason
                        inner.close_position(
                            current_pos,
                            current_price,
                            current_time,
                            fee,
                            margin,
                            close_reason, // Use assigned reason
                        );
                        // Open new position immediately
                        position = match desired_side {
                            OrderSide::Buy => Some(Position::Long(current_price)),
                            OrderSide::Sell => Some(Position::Short(current_price)),
                        };
                        // Don't update equity here, let next candle's update_equity handle it
                    }
                    // No else needed here, update_equity was called earlier if holding & not flipped
                }
            }
        } // End of loop through candles/predictions

        // Close any remaining open position at the end of the backtest period
        if let Some(ref pos) = position {
            let last_candle = candles.last().unwrap();
            let last_price = last_candle.close;
            let last_time = last_candle.close_time;
            debug!("Closing final position {:?} at {}", pos.side(), last_price);
            inner.close_position(
                pos,
                last_price,
                last_time,
                fee,
                margin,
                "End of Test".to_string(), // Reason for final close
            );
        } else {
            // If ending flat, record final equity state
            let last_time = candles.last().unwrap().close_time;
            inner.update_equity_value(inner.cash, last_time);
        }

        // --- Calculate Metrics ---
        let days = days_between(
            candles.first().unwrap().open_time,
            candles.last().unwrap().close_time,
        );
        // Use a minimum of 1 day for monthly calculation to avoid issues with very short tests
        let months = (days.max(1) as f64 / 30.44).max(1.0 / 30.44); // Avoid division by zero, ensure positive

        let total_trades = inner.correct + inner.incorrect;
        let accuracy = if total_trades == 0 {
            0.0
        } else {
            inner.correct as f64 / total_trades as f64
        };
        let win_rate = accuracy; // Alias for now, could be different if trades have varying sizes/risk

        let final_cash = inner.cash;
        // Calculate monthly return based on compound growth
        let monthly_return =
            if months > 0.0 && final_cash.is_finite() && final_cash > STARTING_CASH * 0.01 {
                // Check final cash > 0 to avoid issues with powf on negative numbers
                if final_cash > 0.0 {
                    (final_cash / STARTING_CASH).powf(1.0 / months) - 1.0
                } else {
                    -1.0 // Total loss
                }
            } else {
                if !final_cash.is_finite() {
                    warn!("Final cash is NaN or Infinity during metric calculation.");
                }
                -1.0 // Indicates total loss or invalid state
            };

        // Calculate Sharpe Ratio using equity curve returns for better accuracy
        let period_returns = inner.calculate_period_returns(); // Get returns per candle period
        let sharpe_ratio = if !period_returns.is_empty() {
            let mean_return = period_returns.iter().sum::<f64>() / period_returns.len() as f64;
            let std_dev_return = std_deviation(&period_returns).unwrap_or(0.0);

            if std_dev_return.abs() > f64::EPSILON {
                // Annualize Sharpe Ratio
                // Estimate average period duration in days for annualization factor
                let avg_period_duration_days = if inner.equity_curve.len() > 1 {
                    let total_duration = inner
                        .equity_curve
                        .last()
                        .unwrap()
                        .0
                        .signed_duration_since(inner.equity_curve.first().unwrap().0);
                    // Use num_seconds for better precision with shorter intervals
                    (total_duration.num_seconds() as f64 / 86400.0) // Total duration in days
                        / ((inner.equity_curve.len() - 1) as f64) // Divided by number of periods
                } else {
                    1.0 // Default to 1 day if only one point or calculation fails
                };
                // Ensure duration is positive and reasonable (e.g., at least 1 second)
                let safe_avg_period_duration_days = avg_period_duration_days.max(1.0 / 86400.0);
                let periods_per_year = 365.25 / safe_avg_period_duration_days;

                (mean_return - RISK_FREE_RATE) / std_dev_return * periods_per_year.sqrt()
            } else {
                0.0 // Avoid division by zero if std dev is zero (no volatility)
            }
        } else {
            0.0 // No returns, no Sharpe ratio
        };

        let metrics = TestData {
            accuracy,
            monthly_return,
            final_cash,
            sharpe_ratio,
            max_drawdown: inner.calculate_max_drawdown(),
            total_trades,
            win_rate,
        };

        // --- Return SimulationOutput ---
        Ok(SimulationOutput {
            metrics,
            trade_log: inner.get_trade_log().clone(), // Clone the log
            equity_curve: inner.get_equity_curve().clone(), // Clone the curve
        })
    }

    // --- Metric Aggregation Helpers ---
    pub fn get_accuracies(data: &[&Self]) -> Vec<f64> {
        data.iter()
            .map(|d| d.accuracy)
            .filter(|&v| v.is_finite())
            .collect()
    }
    pub fn get_monthly_returns(data: &[&Self]) -> Vec<f64> {
        data.iter()
            .map(|d| d.monthly_return)
            .filter(|&v| v.is_finite())
            .collect()
    }
    pub fn get_sharpe_ratios(data: &[&Self]) -> Vec<f64> {
        data.iter()
            .map(|d| d.sharpe_ratio)
            .filter(|&v| v.is_finite())
            .collect()
    }
    // Add helpers for other metrics if needed for aggregation (e.g., win_rate, max_drawdown)
}

impl fmt::Display for TestData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Acc: {:.1}%, M_Ret: {:.2}%, Sharpe: {:.2}, MaxDD: {:.1}%, Trades: {}, WinRate: {:.1}%, FinalCash: {:.2}",
            self.accuracy * 100.0,
            self.monthly_return * 100.0,
            self.sharpe_ratio,
            self.max_drawdown * 100.0,
            self.total_trades,
            self.win_rate * 100.0,
            self.final_cash
        )
    }
}

// --- Position Enum ---
#[derive(Debug, Clone, PartialEq)]
pub enum Position {
    Long(f64),  // Entry price
    Short(f64), // Entry price
}

impl Position {
    /// Calculates the return percentage based on current price and entry price.
    fn get_return(&self, current_price: f64) -> f64 {
        let entry_price = self.entry_price();
        if entry_price.abs() < f64::EPSILON {
            return 0.0; // Avoid division by zero
        }
        match *self {
            Position::Long(_) => (current_price - entry_price) / entry_price,
            Position::Short(_) => (entry_price - current_price) / entry_price, // Correct for short
        }
    }
    /// Returns the entry price of the position.
    fn entry_price(&self) -> f64 {
        match *self {
            Position::Long(p) | Position::Short(p) => p,
        }
    }
    /// Returns the side (Buy/Sell) of the position.
    fn side(&self) -> OrderSide {
        match self {
            Position::Long(_) => OrderSide::Buy,
            Position::Short(_) => OrderSide::Sell,
        }
    }
}

// --- Inner Simulation State ---
struct InnerTestData {
    cash: f64,
    correct: u32,                            // Count of profitable trades
    incorrect: u32,                          // Count of losing trades
    equity_curve: Vec<(DateTime<Utc>, f64)>, // Track time with equity
    trade_returns: Vec<f64>,                 // Track returns of closed trades (before fees)
    peak_equity: f64,
    max_drawdown: f64,
    trade_log: Vec<TradeLogEntry>, // Add trade log
    symbol: String,                // Store the symbol being tested
}

impl InnerTestData {
    fn new(starting_cash: f64, symbol: String, start_time: DateTime<Utc>) -> Self {
        Self {
            cash: starting_cash,
            correct: 0,
            incorrect: 0,
            equity_curve: vec![(start_time, starting_cash)], // Start with initial cash and time
            trade_returns: Vec::new(),
            peak_equity: starting_cash,
            max_drawdown: 0.0,
            trade_log: Vec::new(), // Initialize log
            symbol,                // Store symbol
        }
    }

    /// Closes the current position, updates cash, records trade outcome, and updates equity curve.
    fn close_position(
        &mut self,
        position: &Position,
        close_price: f64,
        close_time: DateTime<Utc>, // Add close time
        fee: f64,
        margin: f64,
        reason: String, // Add reason for closing
    ) {
        let entry_price = position.entry_price();
        let trade_return = position.get_return(close_price); // Raw return before fees/margin
        let initial_value = self.cash; // Cash *before* this trade's P/L is applied

        // Calculate P/L based on margin and return
        let cash_change = initial_value * trade_return * margin;

        // Calculate fee based on approximate trade value (entry and exit)
        // This is an approximation; real fees depend on execution price and quantity.
        let approx_trade_value = initial_value * margin; // Value exposed due to margin
        let fee_cost = approx_trade_value.abs() * fee * 2.0; // Fee on entry and exit value approx

        let final_cash = (initial_value + cash_change - fee_cost).max(0.0); // Prevent negative cash

        // Estimate quantity traded based on initial cash and entry price (for logging)
        let quantity_traded = if entry_price.abs() > f64::EPSILON {
            (initial_value * margin / entry_price).abs()
        } else {
            0.0
        };

        // Record trade outcome (based on raw return before fees)
        if trade_return > 0.0 {
            self.correct += 1;
        } else if trade_return < 0.0 {
            self.incorrect += 1;
        }
        self.trade_returns.push(trade_return); // Store raw return

        // Create Log Entry
        let log_entry = TradeLogEntry {
            timestamp: close_time,
            symbol: self.symbol.clone(),
            side: position.side(), // Side of the position being closed
            entry_price,
            exit_price: close_price,
            quantity: quantity_traded,
            pnl: cash_change - fee_cost, // Net PnL including fees and margin effect
            pnl_pct: trade_return - (fee * 2.0), // Approx return % after entry/exit fees (ignoring margin effect here)
            fee: fee_cost,
            cash_after_trade: final_cash,
            equity_after_trade: final_cash, // Equity equals cash when flat
            reason,
        };
        self.trade_log.push(log_entry);

        // Update cash *after* logging
        self.cash = final_cash;

        // Update equity curve *after* closing trade (equity = cash when flat)
        self.update_equity_value(self.cash, close_time);
    }

    /// Updates the equity curve based on the current position and price (for unrealized P/L).
    fn update_equity(
        &mut self,
        position: &Position,
        current_price: f64,
        current_time: DateTime<Utc>,
        margin: f64,
    ) {
        let current_return = position.get_return(current_price);
        // Estimate current equity = cash_at_entry * (1 + unrealized_return * margin)
        // This requires tracking cash_at_entry or calculating based on current cash and realized PnL.
        // Simpler approximation: current_equity = current_cash + unrealized_pnl
        // unrealized_pnl = cash_at_entry * current_return * margin
        // Let's use the simpler approach based on current cash:
        // Assume current cash reflects realized PnL up to this point.
        // The change in equity from current cash is due to the unrealized return on the *current* position's value.
        // Value of current position = self.cash (approx) * margin? No, that's wrong.
        // Let's stick to the definition: Equity = Cash + Position Value - Liabilities
        // Simplified: Equity = Cash * (1 + unrealized_return * margin) - approx open fees? Ignore fees for unrealized.
        let estimated_equity = self.cash * (1.0 + current_return * margin);
        self.update_equity_value(estimated_equity.max(0.0), current_time); // Ensure non-negative equity
    }

    /// Adds a value to the equity curve and updates peak/drawdown.
    fn update_equity_value(&mut self, current_equity: f64, timestamp: DateTime<Utc>) {
        // Sanitize equity value
        let equity = if current_equity.is_finite() && current_equity >= 0.0 {
            current_equity
        } else {
            warn!(
                "Invalid equity value ({}) calculated at {}. Setting to 0.0.",
                current_equity, timestamp
            );
            0.0
        };

        // Avoid adding redundant points if time and equity haven't changed significantly
        if let Some((last_time, last_equity)) = self.equity_curve.last() {
            if *last_time == timestamp && (last_equity - equity).abs() < f64::EPSILON {
                return;
            }
            // Also avoid adding points if time hasn't advanced
            if *last_time >= timestamp && self.equity_curve.len() > 1 {
                debug!(
                    "Attempted to add equity point at or before last timestamp: {} vs {}",
                    timestamp, last_time
                );
                return; // Don't add out-of-order points
            }
        }

        self.equity_curve.push((timestamp, equity));

        // Update peak equity and max drawdown
        self.peak_equity = self.peak_equity.max(equity);
        if self.peak_equity > f64::EPSILON {
            // Drawdown is calculated relative to the peak equity seen so far
            let drawdown = (self.peak_equity - equity) / self.peak_equity;
            self.max_drawdown = self.max_drawdown.max(drawdown);
        }
    }

    /// Calculates returns for each period (using equity curve). Needed for proper Sharpe.
    fn calculate_period_returns(&self) -> Vec<f64> {
        if self.equity_curve.len() < 2 {
            return Vec::new();
        }
        self.equity_curve
            .windows(2)
            .map(|w| {
                let prev_equity = w[0].1;
                let curr_equity = w[1].1;
                // Calculate return based on previous equity
                if prev_equity.abs() > f64::EPSILON {
                    (curr_equity - prev_equity) / prev_equity
                } else {
                    0.0 // Avoid division by zero if previous equity was zero
                }
            })
            .collect()
    }

    /// Returns the maximum drawdown calculated during the simulation.
    fn calculate_max_drawdown(&self) -> f64 {
        self.max_drawdown
    }
    /// Returns a reference to the trade log.
    fn get_trade_log(&self) -> &Vec<TradeLogEntry> {
        &self.trade_log
    }
    /// Returns a reference to the equity curve.
    fn get_equity_curve(&self) -> &Vec<(DateTime<Utc>, f64)> {
        &self.equity_curve
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::candlestick::CandlestickBuilder;
    use chrono::Utc;

    fn create_cfg(fee: f64, margin: f64, sl: Option<f64>, tp: Option<f64>) -> KryptoConfig {
        KryptoConfig {
            fee: Some(fee),
            margin,
            trade_stop_loss_percentage: sl,
            trade_take_profit_percentage: tp,
            ..KryptoConfig::default() // Use default for other fields
        }
    }

    fn create_candle(time_offset: i64, close: f64) -> Candlestick {
        let base_time = Utc::now(); // Use a fixed base time for consistency
        CandlestickBuilder::default()
            .open_time(base_time + chrono::Duration::seconds(time_offset))
            .close_time(base_time + chrono::Duration::seconds(time_offset + 60)) // Assume 1-min candles for simplicity
            .open(close) // Simplification: O=H=L=C=close
            .high(close)
            .low(close)
            .close(close)
            .volume(100.0)
            .build()
            .unwrap()
    }

    #[test]
    fn test_basic_long_win_simulation() {
        let cfg = create_cfg(0.001, 1.0, None, None); // 0.1% fee, 1x margin
        let predictions = vec![1.0, 1.0]; // Go long, stay long
        let candles = vec![
            create_candle(0, 100.0),  // Entry candle
            create_candle(60, 110.0), // Exit candle
        ];
        let output = TestData::run_simulation("TEST", &predictions, &candles, &cfg).unwrap();
        let result = output.metrics;

        assert!(result.accuracy > 0.99); // 1 correct trade / 1 total trade
        assert_eq!(result.total_trades, 1);
        // Expected return: (110 - 100) / 100 = 0.1 (10%)
        // Cash change = 1000 * 0.1 * 1.0 = 100
        // Fee = (1000 * 1.0) * 0.001 * 2 = 2.0
        // Final cash = 1000 + 100 - 2.0 = 1098.0
        assert!((result.final_cash - 1098.0).abs() < 0.01);
        assert!(result.max_drawdown < 0.01); // Should be low drawdown
        assert_eq!(output.trade_log.len(), 1);
        assert_eq!(output.equity_curve.len(), 3); // Start, Hold (at t=1), End (at t=2)
    }

    #[test]
    fn test_basic_short_loss_with_margin_simulation() {
        let cfg = create_cfg(0.001, 5.0, None, None); // 0.1% fee, 5x margin
        let predictions = vec![-1.0, -1.0]; // Go short, stay short
        let candles = vec![
            create_candle(0, 100.0),  // Entry candle
            create_candle(60, 105.0), // Exit candle (price went up -> loss for short)
        ];
        let output = TestData::run_simulation("TEST", &predictions, &candles, &cfg).unwrap();
        let result = output.metrics;

        assert!(result.accuracy < 0.01); // 0 correct trades / 1 total trade
        assert_eq!(result.total_trades, 1);
        // Expected return: (100 - 105) / 100 = -0.05 (-5%)
        // Cash change = 1000 * -0.05 * 5.0 = -250
        // Fee = (1000 * 5.0) * 0.001 * 2 = 10.0
        // Final cash = 1000 - 250 - 10.0 = 740.0
        assert!((result.final_cash - 740.0).abs() < 0.01);
        // Drawdown: Peak was 1000. Lowest equity was ~740. Drawdown ~ (1000-740)/1000 = 0.26
        assert!(result.max_drawdown > 0.25 && result.max_drawdown < 0.27);
        assert_eq!(output.trade_log.len(), 1);
        assert_eq!(output.equity_curve.len(), 3);
    }

    #[test]
    fn test_empty_input_error_simulation() {
        let cfg = create_cfg(0.0, 1.0, None, None);
        let result = TestData::run_simulation("TEST", &[], &[], &cfg);
        assert!(matches!(
            result,
            Err(KryptoError::EmptyCandlesOrPredictions)
        ));
    }

    #[test]
    fn test_mismatched_input_error_simulation() {
        let cfg = create_cfg(0.0, 1.0, None, None);
        let predictions = vec![1.0];
        let candles = vec![create_candle(0, 100.0), create_candle(60, 101.0)];
        let result = TestData::run_simulation("TEST", &predictions, &candles, &cfg);
        assert!(matches!(
            result,
            Err(KryptoError::UnequalCandlesAndPredictions(2, 1))
        ));
    }

    #[test]
    fn test_stop_loss_trigger() {
        let cfg = create_cfg(0.001, 1.0, Some(0.05), None); // 5% SL
        let predictions = vec![1.0, 1.0, 1.0]; // Go long
        let candles = vec![
            create_candle(0, 100.0),  // Entry
            create_candle(60, 98.0),  // Price drops (-2%)
            create_candle(120, 94.0), // Price drops further (-6% from entry) -> SL should trigger
        ];
        let output = TestData::run_simulation("TEST", &predictions, &candles, &cfg).unwrap();
        let result = output.metrics;

        assert_eq!(result.total_trades, 1);
        // SL triggered at 94.0. Return = (94-100)/100 = -0.06
        // Cash change = 1000 * -0.06 * 1.0 = -60
        // Fee = (1000 * 1.0) * 0.001 * 2 = 2.0
        // Final cash = 1000 - 60 - 2.0 = 938.0
        assert!((result.final_cash - 938.0).abs() < 0.01);
        assert_eq!(output.trade_log.len(), 1);
        assert_eq!(output.trade_log[0].reason, "Stop Loss");
        assert_eq!(output.equity_curve.len(), 4); // Start, Hold@98, Close@94, End(flat)
    }

    #[test]
    fn test_take_profit_trigger() {
        let cfg = create_cfg(0.001, 1.0, None, Some(0.08)); // 8% TP
        let predictions = vec![1.0, 1.0, 1.0]; // Go long
        let candles = vec![
            create_candle(0, 100.0),   // Entry
            create_candle(60, 105.0),  // Price rises (+5%)
            create_candle(120, 109.0), // Price rises further (+9% from entry) -> TP should trigger
        ];
        let output = TestData::run_simulation("TEST", &predictions, &candles, &cfg).unwrap();
        let result = output.metrics;

        assert_eq!(result.total_trades, 1);
        // TP triggered at 109.0. Return = (109-100)/100 = 0.09
        // Cash change = 1000 * 0.09 * 1.0 = 90
        // Fee = (1000 * 1.0) * 0.001 * 2 = 2.0
        // Final cash = 1000 + 90 - 2.0 = 1088.0
        assert!((result.final_cash - 1088.0).abs() < 0.01);
        assert_eq!(output.trade_log.len(), 1);
        assert_eq!(output.trade_log[0].reason, "Take Profit");
        assert_eq!(output.equity_curve.len(), 4); // Start, Hold@105, Close@109, End(flat)
    }

    #[test]
    fn test_signal_flip() {
        let cfg = create_cfg(0.001, 1.0, None, None);
        let predictions = vec![1.0, -1.0, -1.0]; // Go long, then flip short
        let candles = vec![
            create_candle(0, 100.0),   // Entry long
            create_candle(60, 102.0),  // Flip short here (+2% gain on long)
            create_candle(120, 101.0), // Exit short here (+1% gain on short)
        ];
        let output = TestData::run_simulation("TEST", &predictions, &candles, &cfg).unwrap();
        let result = output.metrics;

        assert_eq!(result.total_trades, 2);
        // Trade 1 (Long): Closed at 102.0. Return = 0.02. PnL = 1000*0.02*1 = 20. Fee = 2. Cash after = 1000+20-2 = 1018.
        // Trade 2 (Short): Entry 102.0. Closed at 101.0. Return = (102-101)/102 = 0.0098...
        // PnL = 1018 * 0.0098... * 1 = 10. Fee = 1018*1*0.001*2 = 2.036
        // Final Cash = 1018 + 10 - 2.036 = 1025.964
        assert!((result.final_cash - 1025.96).abs() < 0.01); // Check final cash
        assert_eq!(output.trade_log.len(), 2);
        assert_eq!(output.trade_log[0].reason, "Signal Flip");
        assert_eq!(output.trade_log[1].reason, "End of Test");
        assert_eq!(output.equity_curve.len(), 4); // Start, CloseLong/OpenShort@102, CloseShort@101, End(flat)
    }
}
