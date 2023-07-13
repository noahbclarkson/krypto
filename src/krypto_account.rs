use std::error::Error;

use binance::{
    api::Binance,
    futures::{
        account::FuturesAccount,
        general::FuturesGeneral,
        market::FuturesMarket,
        rest_model::{AccountBalance, ExchangeInformation, Filters, Symbol},
    },
    rest_model::OrderSide,
};
use getset::Getters;

use crate::config::Config;

pub struct KryptoAccount {
    pub account: FuturesAccount,
    pub general: FuturesGeneral,
    pub market: FuturesMarket,
    pub exchange_info: Option<ExchangeInformation>,
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

pub struct Order {
    pub symbol: String,
    pub side: OrderSide,
    pub quantity: f32,
}

impl KryptoAccount {
    pub fn new(config: &Config) -> Self {
        let market: FuturesMarket =
            Binance::new(config.api_key().clone(), config.api_secret().clone());
        let account: FuturesAccount =
            Binance::new(config.api_key().clone(), config.api_secret().clone());
        let general: FuturesGeneral =
            Binance::new(config.api_key().clone(), config.api_secret().clone());
        Self {
            account,
            general,
            market,
            exchange_info: None,
        }
    }

    pub async fn precision(&mut self, ticker: String) -> Result<PrecisionData, Box<dyn Error>> {
        let symbol = self.get_symbol(ticker.clone()).await?;
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
        Ok(PrecisionData {
            ticker,
            tick_size: *ts.unwrap(),
            step_size: *ss.unwrap(),
            tick_precision: ts.unwrap().log10().abs() as usize,
            step_precision: ss.unwrap().log10().abs() as usize,
        })
    }

    async fn get_symbol(&mut self, ticker: String) -> Result<Symbol, Box<dyn Error>> {
        if self.exchange_info.is_none() {
            self.exchange_info = Some(
                self.general
                    .exchange_info()
                    .await
                    .map_err(|e| Box::new(KryptoError::ExchangeInfoError(e.to_string())))?,
            );
        }
        let symbol = self
            .exchange_info
            .as_ref()
            .unwrap()
            .symbols
            .iter()
            .find(|symbol| symbol.symbol == ticker);
        if symbol.is_none() {
            return Err(Box::new(KryptoError::InvalidSymbol(ticker)));
        }
        Ok(symbol.unwrap().clone())
    }

    pub async fn extract_base(&mut self, ticker: String) -> Result<String, Box<dyn Error>> {
        let symbol = self.get_symbol(ticker).await?;
        Ok(symbol.base_asset)
    }

    pub async fn extract_quote(&mut self, ticker: String) -> Result<String, Box<dyn Error>> {
        let symbol = self.get_symbol(ticker).await?;
        Ok(symbol.quote_asset)
    }

    pub async fn get_price(&mut self, ticker: String) -> Result<f32, Box<dyn Error>> {
        let symbol = self.get_symbol(ticker).await?;
        let price = self
            .market
            .get_price(&symbol.symbol)
            .await
            .map_err(|e| Box::new(KryptoError::PriceError(e.to_string())))?;
        Ok(price.price as f32)
    }

    pub async fn order(&mut self, order: Order) -> Result<f64, Box<dyn Error>> {
        let symbol = self.get_symbol(order.symbol.clone()).await?;
        let qty = self
            .precision(order.symbol.clone())
            .await?
            .fmt_quantity(order.quantity as f64)?;
        let result = match order.side {
            OrderSide::Buy => self.account.market_buy(symbol.symbol, qty).await,
            OrderSide::Sell => self.account.market_sell(symbol.symbol, qty).await,
        };
        if result.is_err() {
            return Err(Box::new(KryptoError::OrderError(
                result.err().unwrap().to_string(),
            )));
        }
        Ok(qty)
    }

    pub async fn set_default_leverages(&mut self, config: &Config) -> Result<(), Box<dyn Error>> {
        let blacklist = config.blacklist().clone().unwrap_or_default();
        for ticker in config.tickers() {
            if blacklist.contains(ticker) {
                continue;
            }
            self.set_leverage(ticker, *config.leverage()).await?;
        }
        Ok(())
    }

    pub async fn set_leverage(
        &mut self,
        ticker: &String,
        leverage: u8,
    ) -> Result<(), Box<dyn Error>> {
        let symbol = self.get_symbol(ticker.clone()).await?;
        self.account
            .change_initial_leverage(symbol.symbol, leverage)
            .await
            .map_err(|e| Box::new(KryptoError::LeverageError(e.to_string())))?;
        Ok(())
    }

    pub async fn get_total_balance_in(&mut self, asset: &str) -> Result<f32, Box<dyn Error>> {
        let balances: Vec<AccountBalance> = self
            .account
            .account_balance()
            .await
            .map_err(|e| Box::new(KryptoError::BalanceError(e.to_string())))?;
        let mut total = 0.0 as f32;
        for balance in balances {
            if balance.available_balance == 0.0 {
                continue;
            }
            if balance.asset == asset {
                total += balance.available_balance as f32;
            } else {
                let price = self
                    .get_price(format!("{}{}", balance.asset, asset))
                    .await?;
                total += balance.available_balance as f32 * price;
            }
        }
        Ok(total)
    }

    pub async fn get_balance(&mut self, asset: &str) -> Result<f32, Box<dyn Error>> {
        let balances: Vec<AccountBalance> = self
            .account
            .account_balance()
            .await
            .map_err(|e| Box::new(KryptoError::BalanceError(e.to_string())))?;
        let balance = balances.iter().find(|balance| balance.asset == asset);
        if balance.is_none() {
            return Err(Box::new(KryptoError::BalanceError(format!(
                "Unable to find balance for {}",
                asset
            ))));
        }
        let balance = balance.unwrap();
        Ok(balance.available_balance as f32)
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

#[derive(thiserror::Error, Debug)]
pub enum KryptoError {
    #[error("Invalid symbol: {0}")]
    InvalidSymbol(String),
    #[error("Unable to retrieve exchange information: {0}")]
    ExchangeInfoError(String),
    #[error("Unable to retrieve price: {0}")]
    PriceError(String),
    #[error("Error sending order: {0}")]
    OrderError(String),
    #[error("Error updating default leverage: {0}")]
    LeverageError(String),
    #[error("Error retrieving balance: {0}")]
    BalanceError(String),
}
