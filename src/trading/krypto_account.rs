use binance::{
    account::Account,
    general::General,
    margin::Margin,
    market::Market,
    rest_model::{
        ExchangeInformation, Filters, IsolatedMarginAccountDetails, MarginOrder, MarginOrderResult,
        OrderSide, OrderType, SideEffectType, Symbol,
    },
};
use chrono::Utc;
use tracing::{error, info, instrument, warn}; // Added warn

use crate::{config::KryptoConfig, error::KryptoError};

use super::precision_data::PrecisionData;

#[derive(Clone)] // Removed Debug derive as it might expose sensitive info if printed
pub struct KryptoAccount {
    pub margin: Margin,
    pub general: General,
    pub market: Market,
    pub account: Account,
    pub symbol: String,
    // Cache precision data after fetching once
    precision_data: Option<PrecisionData>,
}

impl KryptoAccount {
    pub async fn new(config: &KryptoConfig, symbol: String) -> Result<Self, KryptoError> {
        // Validate symbol format roughly?
        if symbol.is_empty()
            || !symbol
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        {
            return Err(KryptoError::ConfigError(format!(
                "Invalid symbol format: {}",
                symbol
            )));
        }

        let margin = config.get_binance::<Margin>();
        let general = config.get_binance::<General>();
        let market = config.get_binance::<Market>();
        let account = config.get_binance::<Account>();

        let mut instance = Self {
            margin,
            general,
            market,
            account,
            symbol: symbol.clone(),
            precision_data: None,
        };

        // Pre-fetch and cache precision data on creation
        instance.get_precision_data().await?;
        info!("KryptoAccount initialized for symbol: {}", symbol);
        Ok(instance)
    }

    #[instrument(skip(self))]
    pub async fn exchange_info(&self) -> Result<ExchangeInformation, KryptoError> {
        self.general.exchange_info().await.map_err(|e| {
            KryptoError::BinanceApiError(format!("Failed to get exchange information: {}", e))
        })
    }

    #[instrument(skip(self))]
    pub async fn get_symbol_info(&self) -> Result<Symbol, KryptoError> {
        // Consider caching exchange info for a short duration if called frequently
        let info = self.exchange_info().await?;
        info.symbols
            .into_iter() // Use into_iter to consume and find efficiently
            .find(|s| s.symbol == self.symbol)
            .ok_or_else(|| KryptoError::SymbolNotFound(self.symbol.clone()))
    }

    #[instrument(skip(self))]
    pub async fn get_precision_data(&mut self) -> Result<PrecisionData, KryptoError> {
        if let Some(pd) = &self.precision_data {
            return Ok(pd.clone());
        }

        info!("Fetching precision data for {}", self.symbol);
        let symbol_info = self.get_symbol_info().await?;
        let mut tick_size = None;
        let mut step_size = None;

        for filter in &symbol_info.filters {
            match filter {
                Filters::PriceFilter { tick_size: ts, .. } => tick_size = Some(*ts),
                Filters::LotSize { step_size: ss, .. } => step_size = Some(*ss),
                _ => {} // Ignore other filters
            }
        }

        let ts = tick_size.ok_or_else(|| {
            KryptoError::BinanceApiError(format!(
                "PriceFilter (tick_size) not found for symbol {}",
                self.symbol
            ))
        })?;
        let ss = step_size.ok_or_else(|| {
            KryptoError::BinanceApiError(format!(
                "LotSize (step_size) not found for symbol {}",
                self.symbol
            ))
        })?;

        let pd = PrecisionData::new(symbol_info.symbol.clone(), ts, ss)?; // Use new which returns Result
        self.precision_data = Some(pd.clone());
        Ok(pd)
    }

    pub async fn base_asset(&self) -> Result<String, KryptoError> {
        // Use cached precision data if available
        if let Some(_pd) = &self.precision_data {
            // Need to store base/quote in PrecisionData or fetch symbol_info again
            // Let's fetch symbol_info for now
            Ok(self.get_symbol_info().await?.base_asset)
        } else {
            // Should not happen if get_precision_data is called in new()
            warn!("Precision data not cached when calling base_asset");
            Ok(self.get_symbol_info().await?.base_asset)
        }
    }

    pub async fn quote_asset(&self) -> Result<String, KryptoError> {
        if let Some(_pd) = &self.precision_data {
            Ok(self.get_symbol_info().await?.quote_asset)
        } else {
            warn!("Precision data not cached when calling quote_asset");
            Ok(self.get_symbol_info().await?.quote_asset)
        }
    }

    /// Gets the maximum amount of the base asset that can be borrowed for isolated margin.
    #[instrument(skip(self))]
    pub async fn max_borrowable(&self) -> Result<f64, KryptoError> {
        let base_asset = self.base_asset().await?;
        let asset = self
            .margin
            .max_borrowable(&base_asset, Some(self.symbol.clone())) // Use borrow operator
            .await
            .map_err(|e| {
                KryptoError::BinanceApiError(format!(
                    "Failed to get max borrowable for {}: {}",
                    base_asset, e
                ))
            })?;
        Ok(asset.amount)
    }

    /// Gets the current quantity of the base asset held (free + locked/borrowed?) in the isolated margin account.
    /// Note: This might include borrowed amounts. Check `borrowed` vs `free`.
    #[instrument(skip(self))]
    pub async fn base_asset_balance(&self) -> Result<f64, KryptoError> {
        let details = self.isolated_details().await?;
        // Find the specific asset details for the current symbol
        let asset_details = details.assets.first().ok_or_else(|| {
            KryptoError::BinanceApiError(format!(
                "No isolated asset details found for {}",
                self.symbol
            ))
        })?;

        // Total balance = free + locked (locked might be part of an open order)
        let total_balance = asset_details.base_asset.free + asset_details.base_asset.locked;
        Ok(total_balance)
    }

    /// Gets the amount of base asset currently borrowed.
    #[instrument(skip(self))]
    pub async fn base_asset_borrowed(&self) -> Result<f64, KryptoError> {
        let details = self.isolated_details().await?;
        let asset_details = details.assets.first().ok_or_else(|| {
            KryptoError::BinanceApiError(format!(
                "No isolated asset details found for {}",
                self.symbol
            ))
        })?;
        Ok(asset_details.base_asset.borrowed)
    }

    /// Gets the net position (balance - borrowed). Positive means net long, negative means net short (implicitly).
    #[instrument(skip(self))]
    pub async fn net_base_asset_position(&self) -> Result<f64, KryptoError> {
        let balance = self.base_asset_balance().await?;
        let borrowed = self.base_asset_borrowed().await?;
        Ok(balance - borrowed)
    }

    #[instrument(skip(self))]
    pub async fn isolated_details(&self) -> Result<IsolatedMarginAccountDetails, KryptoError> {
        self.margin
            .isolated_details(Some(vec![self.symbol.clone()]))
            .await
            .map_err(|e| {
                KryptoError::BinanceApiError(format!(
                    "Failed to get isolated details for {}: {}",
                    self.symbol, e
                ))
            })
    }

    /// Places a margin trade order.
    ///
    /// # Arguments
    /// * `side` - `OrderSide::Buy` or `OrderSide::Sell`.
    /// * `reduce_only` - If true, calculates quantity to close the current net position. If false, calculates quantity based on `percentage_of_max_borrow`.
    /// * `percentage_of_max_borrow` - Optional percentage (0.0 to 1.0) of max borrowable amount to use for new positions. Defaults to a configured value if `None`.
    /// * `config` - KryptoConfig needed for default percentage.
    ///
    /// # Returns
    /// The result of the margin order placement.
    #[instrument(skip(self, config))]
    pub async fn make_trade(
        &mut self,
        side: OrderSide,
        reduce_only: bool,
        percentage_of_max_borrow: Option<f64>,
        config: &KryptoConfig, // Pass config
    ) -> Result<MarginOrderResult, KryptoError> {
        let precision_data = self.get_precision_data().await?; // Ensure precision data is loaded/cached

        let quantity = if reduce_only {
            // Calculate quantity needed to close the current net position.
            // If net position is positive (long), we need to sell that amount.
            // If net position is negative (short), we need to buy back that amount.
            let net_position = self.net_base_asset_position().await?;
            info!(
                "Attempting to reduce position. Net position: {}",
                net_position
            );

            if (side == OrderSide::Sell && net_position <= precision_data.get_step_size()) || // Trying to sell but not long
               (side == OrderSide::Buy && net_position >= -precision_data.get_step_size())
            {
                // Trying to buy but not short
                warn!("Reduce only order requested for side {:?}, but net position is {}. No trade placed.", side, net_position);
                // Return a dummy result or a specific error? Let's return an error.
                return Err(KryptoError::BinanceApiError(format!(
                    "Reduce only order for {:?} invalid with net position {}",
                    side, net_position
                )));
            }
            // Take absolute value and subtract a small amount to ensure it closes fully? Or rely on AutoRepay?
            // Let's use the absolute value. AutoRepay should handle the borrow part.
            net_position.abs()
        } else {
            // Calculate quantity for a new position based on max borrowable.
            let percent = percentage_of_max_borrow.unwrap_or(config.trade_qty_percentage);
            if !(0.0..=1.0).contains(&percent) {
                return Err(KryptoError::ConfigError(format!(
                    "Trade quantity percentage ({}) must be between 0.0 and 1.0",
                    percent
                )));
            }
            let max_borrow = self.max_borrowable().await?;
            max_borrow * percent
        };

        // Ensure quantity is positive and non-zero after calculation
        if quantity <= 0.0 {
            return Err(KryptoError::BinanceApiError(format!(
                "Calculated trade quantity ({}) is zero or negative.",
                quantity
            )));
        }

        // Format quantity according to step size precision
        let formatted_quantity = precision_data.fmt_quantity(quantity)?;

        // Ensure formatted quantity is still positive after potential rounding
        if formatted_quantity <= 0.0 {
            return Err(KryptoError::BinanceApiError(format!(
                "Formatted trade quantity ({}) is zero or negative after applying precision.",
                formatted_quantity
            )));
        }

        // Determine SideEffectType based on whether we are reducing or opening/increasing
        // MARGIN_BUY borrows quote to buy base. AUTO_REPAY sells base to repay quote (or base?). Check docs.
        // SideEffectType::MarginBuy -> Used when buying (increasing long or closing short by buying back)
        // SideEffectType::AutoRepay -> Used when selling (closing long or increasing short)
        let side_effect_type = match side {
            OrderSide::Buy => SideEffectType::MarginBuy, // Borrow quote if needed to buy base
            OrderSide::Sell => SideEffectType::AutoRepay, // Sell base, automatically repay loan if possible
        };

        let new_client_order_id = Some(format!(
            "krypto_{}_{}",
            self.symbol,
            Utc::now().timestamp_millis()
        ));

        let margin_order = MarginOrder {
            symbol: self.symbol.clone(),
            side,
            order_type: OrderType::Market, // Use Market for simplicity, consider Limit later
            quantity: Some(formatted_quantity),
            is_isolated: Some("TRUE".to_string()), // Ensure isolated margin
            side_effect_type,                      // Specify effect
            time_in_force: None,                   // Not needed for Market orders
            price: None,                           // Not needed for Market orders
            quote_order_qty: None,                 // Use base asset quantity
            stop_price: None,                      // Not a stop order
            new_client_order_id,
            iceberg_qty: None,
            new_order_resp_type: binance::rest_model::OrderResponse::Full, // Get full response
        };

        info!(order = ?margin_order, "Placing margin trade");

        let order_result = self.margin.trade(margin_order).await.map_err(|e| {
            error!("Failed to place margin trade: {}", e); // Log error details
            KryptoError::BinanceApiError(format!("Failed to place margin trade: {}", e))
        })?;

        info!(result = ?order_result, "Trade successful");
        Ok(order_result)
    }
}
