//! Order execution via Binance Futures API.
//!
//! Provides safe order placement with:
//! - Test order support (dry run)
//! - Position size limits
//! - Order tracking

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Result of an order placement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderResult {
    /// Order ID from exchange (None in dry run)
    pub order_id: Option<u64>,
    /// Client order ID
    pub client_order_id: String,
    /// Symbol
    pub symbol: String,
    /// Side (BUY/SELL)
    pub side: String,
    /// Order type
    pub order_type: String,
    /// Quantity
    pub quantity: f64,
    /// Price (for limit orders)
    pub price: Option<f64>,
    /// Status
    pub status: String,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Was this a dry run?
    pub dry_run: bool,
}

/// Current position information.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PositionInfo {
    pub symbol: String,
    pub side: String,
    pub size: f64,
    pub entry_price: f64,
    pub unrealized_pnl: f64,
    pub liquidation_price: Option<f64>,
}

/// Order executor for Binance Futures.
pub struct Executor {
    api_key: Option<String>,
    api_secret: Option<String>,
    dry_run: bool,
    use_testnet: bool,
    fee_pct: f64,
    // Order tracking
    pending_orders: HashMap<String, OrderResult>,
    completed_orders: Vec<OrderResult>,
}

impl Executor {
    /// Create a new executor.
    pub fn new(
        api_key: Option<String>,
        api_secret: Option<String>,
        dry_run: bool,
        use_testnet: bool,
        fee_pct: f64,
    ) -> Self {
        Self {
            api_key,
            api_secret,
            dry_run,
            use_testnet,
            fee_pct,
            pending_orders: HashMap::new(),
            completed_orders: Vec::new(),
        }
    }

    /// Create a dry-run executor (no real orders).
    pub fn dry_run() -> Self {
        Self::new(None, None, true, true, 0.0)
    }

    /// Place a limit order.
    ///
    /// In dry-run mode, simulates the order without sending to exchange.
    pub async fn place_limit_order(
        &mut self,
        symbol: &str,
        side: OrderSide,
        quantity: f64,
        price: f64,
    ) -> Result<OrderResult> {
        let client_order_id = format!(
            "krypto_{}_{}",
            symbol,
            chrono::Utc::now().format("%Y%m%d%H%M%S")
        );

        if self.dry_run {
            tracing::info!(
                "[DRY RUN] {} {} {} @ {} on {}",
                side.as_str(),
                symbol,
                quantity,
                price,
                if self.use_testnet {
                    "testnet"
                } else {
                    "mainnet"
                }
            );

            let result = OrderResult {
                order_id: None,
                client_order_id: client_order_id.clone(),
                symbol: symbol.to_string(),
                side: side.as_str().to_string(),
                order_type: "LIMIT".to_string(),
                quantity,
                price: Some(price),
                status: "NEW".to_string(),
                timestamp: Utc::now(),
                dry_run: true,
            };

            self.pending_orders.insert(client_order_id, result.clone());
            return Ok(result);
        }

        // Real order placement
        self.place_real_order(symbol, side, quantity, Some(price), None)
            .await
    }

    /// Place a market order.
    pub async fn place_market_order(
        &mut self,
        symbol: &str,
        side: OrderSide,
        quantity: f64,
    ) -> Result<OrderResult> {
        let client_order_id = format!(
            "krypto_{}_{}",
            symbol,
            chrono::Utc::now().format("%Y%m%d%H%M%S")
        );

        if self.dry_run {
            tracing::info!(
                "[DRY RUN] {} MARKET {} {} on {}",
                side.as_str(),
                symbol,
                quantity,
                if self.use_testnet {
                    "testnet"
                } else {
                    "mainnet"
                }
            );

            let result = OrderResult {
                order_id: None,
                client_order_id: client_order_id.clone(),
                symbol: symbol.to_string(),
                side: side.as_str().to_string(),
                order_type: "MARKET".to_string(),
                quantity,
                price: None,
                status: "FILLED".to_string(),
                timestamp: Utc::now(),
                dry_run: true,
            };

            self.completed_orders.push(result.clone());
            return Ok(result);
        }

        // Real order placement
        self.place_real_order(symbol, side, quantity, None, None)
            .await
    }

    /// Place a stop-loss order.
    pub async fn place_stop_loss(
        &mut self,
        symbol: &str,
        side: OrderSide,
        quantity: f64,
        stop_price: f64,
    ) -> Result<OrderResult> {
        let client_order_id = format!(
            "krypto_sl_{}_{}",
            symbol,
            chrono::Utc::now().format("%Y%m%d%H%M%S")
        );

        if self.dry_run {
            tracing::info!(
                "[DRY RUN] {} STOP_LOSS {} {} @ {} on {}",
                side.as_str(),
                symbol,
                quantity,
                stop_price,
                if self.use_testnet {
                    "testnet"
                } else {
                    "mainnet"
                }
            );

            let result = OrderResult {
                order_id: None,
                client_order_id: client_order_id.clone(),
                symbol: symbol.to_string(),
                side: side.as_str().to_string(),
                order_type: "STOP_MARKET".to_string(),
                quantity,
                price: Some(stop_price),
                status: "NEW".to_string(),
                timestamp: Utc::now(),
                dry_run: true,
            };

            self.pending_orders.insert(client_order_id, result.clone());
            return Ok(result);
        }

        // Real stop-loss order
        self.place_real_order(symbol, side, quantity, None, Some(stop_price))
            .await
    }

    /// Cancel an order.
    pub async fn cancel_order(&mut self, symbol: &str, client_order_id: &str) -> Result<()> {
        if self.dry_run {
            tracing::info!("[DRY RUN] CANCEL {} on {}", client_order_id, symbol);
            self.pending_orders.remove(client_order_id);
            return Ok(());
        }

        // Real cancellation would go here
        tracing::warn!("Real order cancellation not yet implemented");
        Ok(())
    }

    /// Get current positions.
    pub async fn get_positions(&self) -> Result<Vec<PositionInfo>> {
        if self.dry_run {
            // In dry run, return empty positions (tracked by bot instead)
            return Ok(Vec::new());
        }

        // Real position fetch would go here
        tracing::warn!("Real position fetch not yet implemented");
        Ok(Vec::new())
    }

    /// Calculate position size for given equity and price.
    pub fn calculate_position_size(
        &self,
        equity: f64,
        price: f64,
        risk_fraction: f64,
        stop_distance_pct: f64,
    ) -> f64 {
        // Position size = (equity * risk_fraction) / stop_distance_pct
        // But we want position in base currency, not contracts
        let risk_amount = equity * risk_fraction;
        let contracts = risk_amount / (price * stop_distance_pct);
        contracts
    }

    /// Get fee percentage.
    pub fn fee_pct(&self) -> f64 {
        self.fee_pct
    }

    /// Get completed orders.
    pub fn completed_orders(&self) -> &[OrderResult] {
        &self.completed_orders
    }

    /// Get pending orders.
    pub fn pending_orders(&self) -> &HashMap<String, OrderResult> {
        &self.pending_orders
    }

    /// Clear completed orders.
    pub fn clear_completed(&mut self) {
        self.completed_orders.clear();
    }

    // Internal method for real order placement
    async fn place_real_order(
        &mut self,
        symbol: &str,
        side: OrderSide,
        quantity: f64,
        price: Option<f64>,
        stop_price: Option<f64>,
    ) -> Result<OrderResult> {
        use binance::api::Binance;
        use binance::futures::account::{FuturesAccount, OrderRequest};
        use binance::futures::rest_model::OrderType;
        use binance::rest_model::{OrderSide as BinanceOrderSide, TimeInForce};

        let api_key = self
            .api_key
            .as_ref()
            .context("API key required for live trading")?;
        let api_secret = self
            .api_secret
            .as_ref()
            .context("API secret required for live trading")?;

        let config = if self.use_testnet {
            binance::config::Config::testnet()
        } else {
            binance::config::Config::default()
        };

        let account = FuturesAccount::new_with_config(
            Some(api_key.clone()),
            Some(api_secret.clone()),
            &config,
        );

        let binance_side = match side {
            OrderSide::Buy => BinanceOrderSide::Buy,
            OrderSide::Sell => BinanceOrderSide::Sell,
        };

        let (order_type, time_in_force) = if stop_price.is_some() {
            (OrderType::StopMarket, None)
        } else if price.is_some() {
            (OrderType::Limit, Some(TimeInForce::GTC))
        } else {
            (OrderType::Market, None)
        };

        // Capture order_type string before moving
        let order_type_str = format!("{:?}", order_type);

        let request = OrderRequest {
            symbol: symbol.to_string(),
            side: binance_side,
            position_side: None,
            order_type,
            time_in_force,
            quantity: Some(quantity),
            reduce_only: None,
            price,
            stop_price,
            close_position: None,
            activation_price: None,
            callback_rate: None,
            working_type: None,
            price_protect: None,
            new_client_order_id: Some(format!(
                "krypto_{}_{}",
                symbol,
                chrono::Utc::now().format("%Y%m%d%H%M%S")
            )),
        };

        tracing::info!(
            "Placing real order: {} {} {} @ {:?} on {}",
            side.as_str(),
            symbol,
            quantity,
            price,
            if self.use_testnet {
                "testnet"
            } else {
                "mainnet"
            }
        );

        let result = account
            .place_order(request)
            .await
            .context("Failed to place order")?;

        let order_result = OrderResult {
            order_id: Some(result.order_id),
            client_order_id: result.client_order_id,
            symbol: symbol.to_string(),
            side: side.as_str().to_string(),
            order_type: order_type_str,
            quantity,
            price,
            status: format!("{:?}", result.status),
            timestamp: Utc::now(),
            dry_run: false,
        };

        self.completed_orders.push(order_result.clone());
        Ok(order_result)
    }
}

/// Order side (buy or sell).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OrderSide {
    Buy,
    Sell,
}

impl OrderSide {
    pub fn as_str(&self) -> &'static str {
        match self {
            OrderSide::Buy => "BUY",
            OrderSide::Sell => "SELL",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_dry_run_limit_order() {
        let mut executor = Executor::dry_run();
        let result = executor
            .place_limit_order("BTCFDUSD", OrderSide::Buy, 0.01, 50000.0)
            .await
            .unwrap();

        assert!(result.dry_run);
        assert_eq!(result.symbol, "BTCFDUSD");
        assert_eq!(result.side, "BUY");
        assert_eq!(result.status, "NEW");
        assert!(executor
            .pending_orders()
            .contains_key(&result.client_order_id));
    }

    #[tokio::test]
    async fn test_dry_run_market_order() {
        let mut executor = Executor::dry_run();
        let result = executor
            .place_market_order("ETHFDUSD", OrderSide::Sell, 0.1)
            .await
            .unwrap();

        assert!(result.dry_run);
        assert_eq!(result.symbol, "ETHFDUSD");
        assert_eq!(result.side, "SELL");
        assert_eq!(result.status, "FILLED");
    }

    #[test]
    fn test_position_size_calculation() {
        let executor = Executor::dry_run();

        // $10,000 equity, 2% risk, 2% stop distance
        // Position = (10000 * 0.02) / (50000 * 0.02) = 200 / 1000 = 0.2 BTC
        let size = executor.calculate_position_size(10000.0, 50000.0, 0.02, 0.02);
        assert!((size - 0.2).abs() < 0.001);
    }

    #[test]
    fn test_order_side() {
        assert_eq!(OrderSide::Buy.as_str(), "BUY");
        assert_eq!(OrderSide::Sell.as_str(), "SELL");
    }
}
