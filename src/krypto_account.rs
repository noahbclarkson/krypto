use std::{error::Error, sync::Arc};

use binance::{
    account::Account,
    api::Binance,
    general::General,
    margin::Margin,
    market::Market,
    rest_model::{ExchangeInformation, Filters, MarginOrdersQuery},
};
use getset::Getters;
use tokio::sync::Mutex;

use crate::config::Config;

#[derive(Clone)]
pub struct KryptoAccount {
    pub margin: Margin,
    pub general: General,
    pub market: Market,
    pub account: Account,
    exchange_info: Arc<Mutex<Option<ExchangeInformation>>>,
}

#[derive(Debug, Clone, Getters)]
#[getset(get = "pub")]
pub struct PrecisionData {
    ticker: String,
    tick_size: f64,
    step_size: f64,
    tick_precision: usize,
    step_precision: usize,
}

impl KryptoAccount {
    pub fn new(config: &Config) -> Self {
        let mut margin: Margin =
            Binance::new(config.api_key().clone(), config.api_secret().clone());
        margin.recv_window = 10000;
        let general: General = Binance::new(config.api_key().clone(), config.api_secret().clone());
        let market: Market = Binance::new(config.api_key().clone(), config.api_secret().clone());
        let account: Account = Binance::new(config.api_key().clone(), config.api_secret().clone());
        KryptoAccount {
            margin,
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
        let tick_size = *ts.unwrap();
        let step_size = *ss.unwrap();
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

    pub async fn max_borrowable(&mut self, ticker: &str) -> Result<f64, Box<dyn Error>> {
        let base_asset = self.extract_base_asset(ticker).await?;
        let max_borrowable = self.margin.max_borrowable(base_asset, None).await?.amount;
        Ok(max_borrowable)
    }

    pub async fn update_exchange_info(&mut self) -> Result<(), Box<dyn Error>> {
        self.exchange_info
            .lock()
            .await
            .replace(self.general.exchange_info().await?);
        Ok(())
    }

    pub async fn get_balance(&mut self) -> Result<f64, Box<dyn Error>> {
        let account = self.margin.details().await?;
        let total_balance = account.total_net_asset_of_btc;
        let btc_price = self.market.get_price("BTCUSDT").await?.price;
        let total_balance = total_balance * btc_price;
        Ok(total_balance)
    }

    pub async fn close_all_orders(&self, config: &Config) -> Result<(), Box<dyn Error>> {
        for ticker in config.tickers() {
            let orders = self
                .margin
                .orders(MarginOrdersQuery {
                    symbol: ticker.clone(),
                    ..Default::default()
                })
                .await?;
            if orders.len() > 0 {
                println!("{} has {} open orders", ticker, orders.len());
                let result = self.margin.cancel_all_orders(ticker, None).await;
                if result.is_err() {
                    println!("Error (Could not cancel order): {}", result.err().unwrap());
                }
            }
        }
        Ok(())
    }
}

impl PrecisionData {
    pub fn fmt_price_to_string(&self, price: f64) -> String {
        format!("{:.1$}", price, self.tick_precision)
    }

    pub fn fmt_quantity_to_string(&self, quantity: f64) -> String {
        format!("{:.1$}", quantity, self.step_precision)
    }

    pub fn fmt_price(&self, price: f64) -> Result<f64, Box<dyn Error>> {
        Ok(format!("{:.1$}", price, self.tick_precision).parse::<f64>()?)
    }

    pub fn fmt_quantity(&self, quantity: f64) -> Result<f64, Box<dyn Error>> {
        Ok(format!("{:.1$}", quantity, self.step_precision).parse::<f64>()?)
    }
}
