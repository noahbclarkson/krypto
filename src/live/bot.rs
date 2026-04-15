//! Live trading bot.
//!
//! Combines WebSocket feed, strategy, and executor for live trading.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::config::LiveConfig;
use super::executor::{Executor, OrderSide};
use super::feed::{fetch_warmup_data, KlineEvent, LiveFeed};
use crate::paper::{Bar, CompletedTrade, Position};

/// Bot state for monitoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotState {
    /// Bot start time
    pub started_at: Option<DateTime<Utc>>,
    /// Current equity
    pub equity: f64,
    /// Initial capital
    pub initial_capital: f64,
    /// Total PnL
    pub total_pnl: f64,
    /// Total return percentage
    pub total_return_pct: f64,
    /// Number of trades
    pub trades: usize,
    /// Win rate
    pub win_rate: f64,
    /// Current positions by symbol
    pub positions: HashMap<String, PositionInfo>,
    /// Is bot running
    pub is_running: bool,
    /// Last bar time by symbol
    pub last_bar_time: HashMap<String, DateTime<Utc>>,
}

/// Position info for state tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionInfo {
    pub symbol: String,
    pub side: String,
    pub size: f64,
    pub entry_price: f64,
    pub entry_time: DateTime<Utc>,
    pub highest: f64,
    pub lowest: f64,
}

/// Live trading bot.
pub struct LiveBot {
    config: LiveConfig,
    feed: Option<LiveFeed>,
    executor: Executor,
    state: Arc<RwLock<BotState>>,
    // Per-symbol state
    bars: HashMap<String, Vec<Bar>>,
    positions: HashMap<String, Position>,
    // Trade tracking
    completed_trades: Vec<CompletedTrade>,
    gross_profit: f64,
    gross_loss: f64,
}

impl LiveBot {
    /// Create a new live bot with the given configuration.
    pub fn new(config: LiveConfig) -> Result<Self> {
        config.validate()?;

        let executor = Executor::new(
            config.api_key.clone(),
            config.api_secret.clone(),
            config.dry_run,
            config.use_testnet,
            config.fee_pct,
        );

        let state = Arc::new(RwLock::new(BotState {
            started_at: None,
            equity: config.initial_capital,
            initial_capital: config.initial_capital,
            total_pnl: 0.0,
            total_return_pct: 0.0,
            trades: 0,
            win_rate: 0.0,
            positions: HashMap::new(),
            is_running: false,
            last_bar_time: HashMap::new(),
        }));

        Ok(Self {
            config,
            feed: None,
            executor,
            state,
            bars: HashMap::new(),
            positions: HashMap::new(),
            completed_trades: Vec::new(),
            gross_profit: 0.0,
            gross_loss: 0.0,
        })
    }

    /// Get bot state for monitoring.
    pub async fn state(&self) -> BotState {
        self.state.read().await.clone()
    }

    /// Start the bot.
    ///
    /// This will:
    /// 1. Fetch warmup data for each symbol
    /// 2. Connect to WebSocket feed
    /// 3. Process incoming bars and execute trades
    pub async fn start(&mut self) -> Result<()> {
        tracing::info!(
            "Starting live bot for {} symbols",
            self.config.symbols.len()
        );

        // Initialize state
        {
            let mut state = self.state.write().await;
            state.started_at = Some(Utc::now());
            state.is_running = true;
        }

        // Fetch warmup data for each symbol
        for symbol in &self.config.symbols.clone() {
            tracing::info!("Fetching warmup data for {}", symbol);
            let warmup = fetch_warmup_data(
                symbol,
                &self.config.interval,
                50, // Enough for Bollinger calculation
            )
            .await
            .context("Failed to fetch warmup data")?;

            self.bars.insert(symbol.clone(), warmup);
        }

        // Create and start WebSocket feed
        let feed = LiveFeed::new(self.config.symbols.clone(), self.config.interval.clone());
        let mut receiver = feed.subscribe();

        feed.start()
            .await
            .context("Failed to start WebSocket feed")?;
        self.feed = Some(feed);

        tracing::info!("Live bot started, processing bars...");

        // Process incoming bars
        while let Ok(event) = receiver.recv().await {
            if let Err(e) = self.process_bar(&event).await {
                tracing::error!("Error processing bar: {}", e);
            }
        }

        Ok(())
    }

    /// Stop the bot.
    pub async fn stop(&mut self) {
        tracing::info!("Stopping live bot");

        if let Some(feed) = &self.feed {
            feed.stop();
        }

        let mut state = self.state.write().await;
        state.is_running = false;
    }

    /// Process a single bar from the WebSocket.
    async fn process_bar(&mut self, event: &KlineEvent) -> Result<()> {
        let symbol = &event.symbol;
        let kline = &event.kline;

        // Convert to bar
        let bar = kline.to_bar()?;

        // Update bar history
        if let Some(bars) = self.bars.get_mut(symbol) {
            bars.push(bar.clone());
            if bars.len() > 100 {
                bars.remove(0);
            }
        } else {
            self.bars.insert(symbol.clone(), vec![bar.clone()]);
        }

        // Update state
        {
            let mut state = self.state.write().await;
            state.last_bar_time.insert(symbol.clone(), bar.time);
        }

        // Get current position for this symbol
        let position = self.positions.get(symbol).copied().unwrap_or_default();

        // Generate signal
        let signal = self.generate_signal(symbol, &bar, position);

        // Execute signal
        if let Some(trade) = signal {
            self.execute_signal(symbol, trade, &bar).await?;
        }

        // Check trailing stop if in position
        if let Some(trade) = self.check_trailing_stop(symbol, &bar) {
            self.execute_signal(symbol, trade, &bar).await?;
        }

        Ok(())
    }

    /// Generate trading signal using Bollinger Band reversion.
    fn generate_signal(
        &self,
        symbol: &str,
        bar: &Bar,
        position: Position,
    ) -> Option<super::executor::OrderSide> {
        let bars = self.bars.get(symbol)?;
        if bars.len() < self.config.bb_period {
            return None;
        }

        // Calculate Bollinger Bands
        let (upper, lower, _) = self.calculate_bollinger(bars)?;

        // Mean reversion signal
        match position {
            Position::Flat => {
                // Price closed below lower band -> go long
                if bar.close < lower {
                    tracing::debug!(
                        "[{}] Signal: LONG (close {} < lower {})",
                        symbol,
                        bar.close,
                        lower
                    );
                    return Some(OrderSide::Buy);
                }
                // Price closed above upper band -> go short
                if bar.close > upper {
                    tracing::debug!(
                        "[{}] Signal: SHORT (close {} > upper {})",
                        symbol,
                        bar.close,
                        upper
                    );
                    return Some(OrderSide::Sell);
                }
            }
            Position::Long { .. } => {
                // Exit long when price returns to band
                if bar.close >= lower {
                    tracing::debug!(
                        "[{}] Signal: CLOSE LONG (close {} >= lower {})",
                        symbol,
                        bar.close,
                        lower
                    );
                    return Some(OrderSide::Sell);
                }
            }
            Position::Short { .. } => {
                // Exit short when price returns to band
                if bar.close <= upper {
                    tracing::debug!(
                        "[{}] Signal: CLOSE SHORT (close {} <= upper {})",
                        symbol,
                        bar.close,
                        upper
                    );
                    return Some(OrderSide::Buy);
                }
            }
        }

        None
    }

    /// Calculate Bollinger Bands.
    fn calculate_bollinger(&self, bars: &[Bar]) -> Option<(f64, f64, f64)> {
        if bars.len() < self.config.bb_period {
            return None;
        }

        let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
        let recent: &[f64] = &closes[closes.len() - self.config.bb_period..];

        let mean = recent.iter().sum::<f64>() / recent.len() as f64;
        let variance = recent.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / recent.len() as f64;
        let std = variance.sqrt();

        let upper = mean + self.config.bb_std * std;
        let lower = mean - self.config.bb_std * std;

        Some((upper, lower, mean))
    }

    /// Calculate ATR for trailing stop.
    fn calculate_atr(&self, bars: &[Bar]) -> Option<f64> {
        if bars.len() < 14 {
            return None;
        }

        let recent = &bars[bars.len() - 14..];
        let tr_sum: f64 = recent
            .iter()
            .map(|b| b.high - b.low) // Simplified TR
            .sum();

        Some(tr_sum / 14.0)
    }

    /// Check trailing stop.
    fn check_trailing_stop(&self, symbol: &str, bar: &Bar) -> Option<super::executor::OrderSide> {
        let position = self.positions.get(symbol)?;
        let bars = self.bars.get(symbol)?;

        match position {
            Position::Long {
                entry_price,
                highest,
                ..
            } => {
                let atr = self.calculate_atr(bars)?;
                let stop_distance = atr * self.config.atr_stop_mult;
                let stop_price = highest - stop_distance;

                if bar.low <= stop_price {
                    tracing::info!(
                        "[{}] Trailing stop hit: LOW {} <= STOP {} (entry {})",
                        symbol,
                        bar.low,
                        stop_price,
                        entry_price
                    );
                    return Some(OrderSide::Sell);
                }
            }
            Position::Short {
                entry_price,
                lowest,
                ..
            } => {
                let atr = self.calculate_atr(bars)?;
                let stop_distance = atr * self.config.atr_stop_mult;
                let stop_price = lowest + stop_distance;

                if bar.high >= stop_price {
                    tracing::info!(
                        "[{}] Trailing stop hit: HIGH {} >= STOP {} (entry {})",
                        symbol,
                        bar.high,
                        stop_price,
                        entry_price
                    );
                    return Some(OrderSide::Buy);
                }
            }
            Position::Flat => {}
        }

        None
    }

    /// Execute a trading signal.
    async fn execute_signal(&mut self, symbol: &str, side: OrderSide, bar: &Bar) -> Result<()> {
        let position = self.positions.get(symbol).copied().unwrap_or_default();

        match (&position, side) {
            // Open long
            (Position::Flat, OrderSide::Buy) => {
                let size = self.config.max_position_size;
                self.executor
                    .place_limit_order(symbol, OrderSide::Buy, size, bar.close)
                    .await?;

                self.positions.insert(
                    symbol.to_string(),
                    Position::Long {
                        entry_price: bar.close,
                        size,
                        highest: bar.high,
                    },
                );

                tracing::info!("[{}] OPENED LONG @ {} (size {})", symbol, bar.close, size);
            }
            // Open short
            (Position::Flat, OrderSide::Sell) => {
                let size = self.config.max_position_size;
                self.executor
                    .place_limit_order(symbol, OrderSide::Sell, size, bar.close)
                    .await?;

                self.positions.insert(
                    symbol.to_string(),
                    Position::Short {
                        entry_price: bar.close,
                        size,
                        lowest: bar.low,
                    },
                );

                tracing::info!("[{}] OPENED SHORT @ {} (size {})", symbol, bar.close, size);
            }
            // Close long
            (
                Position::Long {
                    entry_price, size, ..
                },
                OrderSide::Sell,
            ) => {
                self.executor
                    .place_limit_order(symbol, OrderSide::Sell, *size, bar.close)
                    .await?;

                let pnl_pct = (bar.close - entry_price) / entry_price * 100.0;
                self.record_trade(symbol, true, *entry_price, bar.close, pnl_pct);

                tracing::info!(
                    "[{}] CLOSED LONG @ {} (PnL: {:.2}%)",
                    symbol,
                    bar.close,
                    pnl_pct
                );

                self.positions.remove(symbol);
            }
            // Close short
            (
                Position::Short {
                    entry_price, size, ..
                },
                OrderSide::Buy,
            ) => {
                self.executor
                    .place_limit_order(symbol, OrderSide::Buy, *size, bar.close)
                    .await?;

                let pnl_pct = (entry_price - bar.close) / entry_price * 100.0;
                self.record_trade(symbol, false, *entry_price, bar.close, pnl_pct);

                tracing::info!(
                    "[{}] CLOSED SHORT @ {} (PnL: {:.2}%)",
                    symbol,
                    bar.close,
                    pnl_pct
                );

                self.positions.remove(symbol);
            }
            _ => {
                tracing::debug!(
                    "[{}] No action for position {:?} and side {:?}",
                    symbol,
                    position,
                    side
                );
            }
        }

        // Update state
        self.update_state().await;

        Ok(())
    }

    /// Record a completed trade.
    fn record_trade(
        &mut self,
        _symbol: &str,
        is_long: bool,
        entry_price: f64,
        exit_price: f64,
        pnl_pct: f64,
    ) {
        let equity = self.config.initial_capital; // Simplified
        let pnl = equity * (pnl_pct / 100.0);

        if pnl > 0.0 {
            self.gross_profit += pnl;
        } else {
            self.gross_loss += pnl.abs();
        }

        let trade = CompletedTrade {
            entry_time: Utc::now(), // Simplified
            exit_time: Utc::now(),
            entry_price,
            exit_price,
            size: 1.0,
            is_long,
            pnl,
            pnl_pct,
        };

        self.completed_trades.push(trade);
    }

    /// Update bot state.
    async fn update_state(&mut self) {
        let mut state = self.state.write().await;

        // Calculate equity (simplified - doesn't track mark-to-market)
        let fees = self.completed_trades.len() as f64
            * 2.0
            * self.config.fee_pct
            * self.config.initial_capital;
        state.equity = self.config.initial_capital + (self.gross_profit - self.gross_loss) - fees;
        state.total_pnl = state.equity - state.initial_capital;
        state.total_return_pct = (state.equity / state.initial_capital - 1.0) * 100.0;
        state.trades = self.completed_trades.len();

        let wins = self.completed_trades.iter().filter(|t| t.pnl > 0.0).count();
        state.win_rate = if state.trades > 0 {
            wins as f64 / state.trades as f64 * 100.0
        } else {
            0.0
        };

        // Update positions
        state.positions.clear();
        for (symbol, pos) in &self.positions {
            let info = match pos {
                Position::Long {
                    entry_price,
                    size,
                    highest,
                } => PositionInfo {
                    symbol: symbol.clone(),
                    side: "LONG".to_string(),
                    size: *size,
                    entry_price: *entry_price,
                    entry_time: Utc::now(),
                    highest: *highest,
                    lowest: 0.0,
                },
                Position::Short {
                    entry_price,
                    size,
                    lowest,
                } => PositionInfo {
                    symbol: symbol.clone(),
                    side: "SHORT".to_string(),
                    size: *size,
                    entry_price: *entry_price,
                    entry_time: Utc::now(),
                    highest: 0.0,
                    lowest: *lowest,
                },
                Position::Flat => continue,
            };
            state.positions.insert(symbol.clone(), info);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bot_creation() {
        let config = LiveConfig::default();
        let bot = LiveBot::new(config);
        assert!(bot.is_ok());
    }

    #[test]
    fn test_config_validation() {
        let config = LiveConfig {
            symbols: vec![],
            ..Default::default()
        };
        assert!(LiveBot::new(config).is_err());
    }

    #[tokio::test]
    async fn test_initial_state() {
        let config = LiveConfig::default();
        let bot = LiveBot::new(config).unwrap();
        let state = bot.state().await;

        assert_eq!(state.initial_capital, 10_000.0);
        assert!(!state.is_running);
        assert_eq!(state.trades, 0);
    }

    #[test]
    fn test_bollinger_calculation() {
        let config = LiveConfig::default();
        let bot = LiveBot::new(config).unwrap();

        let bars: Vec<Bar> = (0..20)
            .map(|i| {
                Bar::new(
                    Utc::now(),
                    100.0 + i as f64,
                    101.0 + i as f64,
                    99.0 + i as f64,
                    100.0 + i as f64,
                    1000.0,
                )
            })
            .collect();

        let (upper, lower, middle) = bot.calculate_bollinger(&bars).unwrap();
        assert!(upper > middle);
        assert!(lower < middle);
    }
}
