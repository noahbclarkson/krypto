use binance::{
    account::Account,
    general::General,
    margin::Margin,
    market::Market,
    rest_model::{
        ExchangeInformation, Filters, MarginOrder, MarginOrderResult, OrderSide,
        OrderType, SideEffectType, Symbol,
    },
};
use chrono::Utc;
use tracing::info;

use crate::{config::KryptoConfig, error::KryptoError};

use super::precision_data::PrecisionData;

#[derive(Clone)]
pub struct KryptoAccount {
    pub margin: Margin,
    pub general: General,
    pub market: Market,
    pub account: Account,
    pub symbol: String,
    pub precision_data: Option<PrecisionData>,
}

impl KryptoAccount {
    pub async fn new(config: KryptoConfig, symbol: String) -> Self {
        let margin = config.get_binance::<Margin>();
        let general = config.get_binance::<General>();
        let market = config.get_binance::<Market>();
        let account = config.get_binance::<Account>();
        Self {
            margin,
            general,
            market,
            account,
            symbol,
            precision_data: None,
        }
    }

    pub async fn exchange_info(&self) -> Result<ExchangeInformation, KryptoError> {
        self.general.exchange_info().await.map_err(|e| {
            KryptoError::BinanceApiError(format!("Failed to get exchange information: {}", e))
        })
    }

    pub async fn get_symbol(&self) -> Result<Symbol, KryptoError> {
        self.exchange_info()
            .await?
            .symbols
            .iter()
            .find(|s| s.symbol == self.symbol)
            .cloned()
            .ok_or(KryptoError::SymbolNotFound)
    }

    pub async fn get_precision_data(&mut self) -> Result<PrecisionData, KryptoError> {
        if let Some(pd) = &self.precision_data {
            return Ok(pd.clone());
        }
        let symbol = self.get_symbol().await?;
        let mut ts = None;
        let mut ss = None;
        for filter in &symbol.filters {
            if let Filters::PriceFilter { tick_size, .. } = filter {
                ts = Some(tick_size);
            }
            if let Filters::LotSize { step_size, .. } = filter {
                ss = Some(step_size);
            }
        }
        let tick_size = *ts.unwrap();
        let step_size = *ss.unwrap();
        self.precision_data = Some(PrecisionData::new(
            symbol.symbol.clone(),
            tick_size,
            step_size,
        ));
        self.precision_data = Some(self.precision_data.clone().unwrap());
        Ok(self.precision_data.clone().unwrap())
    }

    pub async fn base_asset(&self) -> Result<String, KryptoError> {
        let symbol = self.get_symbol().await?;
        Ok(symbol.base_asset)
    }

    pub async fn quote_asset(&self) -> Result<String, KryptoError> {
        let symbol = self.get_symbol().await?;
        Ok(symbol.quote_asset)
    }

    pub async fn max_borrowable(&self) -> Result<f64, KryptoError> {
        let base_asset = self.base_asset().await?;
        let asset = self
            .margin
            .max_borrowable(base_asset, Some(self.symbol.clone()))
            .await
            .map_err(|e| {
                KryptoError::BinanceApiError(format!("Failed to get max borrowable: {}", e))
            })?;
        Ok(asset.amount)
    }

    pub async fn base_asset_quantity(&self) -> Result<f64, KryptoError> {
        let assets = self
            .margin
            .isolated_details(Some(vec![self.symbol.clone()]))
            .await
            .map_err(|e| {
                KryptoError::BinanceApiError(format!("Failed to get base asset quantity: {}", e))
            })?;
        let asset = assets.assets[0].clone();
        let quantity = asset.base_asset.borrowed + asset.base_asset.free;
        Ok(quantity)
    }

    pub async fn make_trade(
        &mut self,
        side: &OrderSide,
        remove_position: bool,
        percentage_amount: Option<f64>,
    ) -> Result<MarginOrderResult, KryptoError> {
        let precision_data = self.get_precision_data().await?;
        let quantity = match remove_position {
            false => self.max_borrowable().await? * percentage_amount.unwrap_or(0.85),
            true => self.base_asset_quantity().await? - precision_data.get_step_size(),
        };
        let side_effect_type = match remove_position {
            false => SideEffectType::MarginBuy,
            true => SideEffectType::AutoRepay,
        };

        let quantity = precision_data.fmt_quantity(quantity)?;
        let new_client_order_id = Some(format!("{}-{}", self.symbol, Utc::now().timestamp()));
        let margin_order = MarginOrder {
            symbol: self.symbol.clone(),
            side: side.clone(),
            order_type: OrderType::Market,
            quantity: Some(quantity),
            is_isolated: Some("TRUE".to_string()),
            side_effect_type,
            time_in_force: None,
            price: None,
            quote_order_qty: None,
            stop_price: None,
            new_client_order_id,
            iceberg_qty: None,
            new_order_resp_type: binance::rest_model::OrderResponse::Full,
        };
        info!("Making trade: {:?}", margin_order);
        let order_result =
            self.margin.trade(margin_order).await.map_err(|e| {
                KryptoError::BinanceApiError(format!("Failed to make trade: {}", e))
            })?;
        Ok(order_result)
    }
}
