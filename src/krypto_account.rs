use std::{error::Error, sync::Arc};

use binance::{
    account::Account,
    general::General,
    market::Market,
    model::{ExchangeInformation, Filters},
};
use tokio::sync::Mutex;

use crate::{config::KryptoConfig, KryptoError};

#[derive(Clone)]
pub struct KryptoAccount {
    pub general: General,
    pub market: Market,
    pub account: Account,
    exchange_info: Arc<Mutex<Option<ExchangeInformation>>>,
}

#[derive(Debug, Clone)]
pub struct PrecisionData {
    pub ticker: String,
    pub tick_size: f64,
    pub step_size: f64,
    pub tick_precision: usize,
    pub step_precision: usize,
}

impl KryptoAccount {
    pub fn new(config: &KryptoConfig) -> Self {
        let general: General = config.get_binance();
        let market: Market = config.get_binance();
        let account: Account = config.get_binance();
        KryptoAccount {
            general,
            market,
            account,
            exchange_info: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn get_precision_data(
        &mut self,
        ticker: String,
    ) -> Result<Box<PrecisionData>, Box<dyn Error>> {
        if self.exchange_info.lock().await.is_none() {
            self.update_exchange_info().await?;
        }
        let ei = self.exchange_info.lock().await;
        let symbol = ei
            .as_ref()
            .unwrap()
            .symbols
            .iter()
            .find(|symbol| symbol.symbol == ticker)
            .unwrap();
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
        let tick_size = ts.unwrap().parse::<f64>()?;
        let step_size = ss.unwrap().parse::<f64>()?;
        let tick_precision = tick_size.log10().abs() as usize;
        let step_precision = step_size.log10().abs() as usize;
        Ok(Box::new(PrecisionData {
            ticker,
            tick_size,
            step_size,
            tick_precision,
            step_precision,
        }))
    }

    pub async fn extract_base_asset(&mut self, ticker: &str) -> Result<String, Box<dyn Error>> {
        if self.exchange_info.lock().await.is_none() {
            self.update_exchange_info().await?;
        }
        let ei = self.exchange_info.lock().await;
        let symbol = ei
            .as_ref()
            .unwrap()
            .symbols
            .iter()
            .find(|symbol| symbol.symbol == ticker)
            .unwrap();
        Ok(symbol.base_asset.clone())
    }

    pub async fn update_exchange_info(&mut self) -> Result<(), KryptoError> {
        self.exchange_info
            .lock()
            .await
            .replace(self.general.exchange_info()?);
        Ok(())
    }
}

impl PrecisionData {
    /// Helper function to round down a value to a specified number of decimal places.
    fn round_down(value: f64, decimal_places: usize) -> f64 {
        let factor = 10f64.powi(decimal_places as i32);
        (value * factor).floor() / factor
    }

    pub fn fmt_price_to_string(&self, price: f64) -> String {
        let rounded_price = Self::round_down(price, self.tick_precision);
        format!("{:.*}", self.tick_precision, rounded_price)
    }

    pub fn fmt_quantity_to_string(&self, quantity: f64) -> String {
        let rounded_quantity = Self::round_down(quantity, self.step_precision);
        format!("{:.*}", self.step_precision, rounded_quantity)
    }

    pub fn fmt_price(&self, price: f64) -> Result<f64, Box<dyn Error>> {
        let rounded_price = Self::round_down(price, self.tick_precision);
        Ok(rounded_price)
    }

    pub fn fmt_quantity(&self, quantity: f64) -> Result<f64, Box<dyn Error>> {
        let rounded_quantity = Self::round_down(quantity, self.step_precision);
        Ok(rounded_quantity)
    }
}