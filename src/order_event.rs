use std::error::Error;

use binance::rest_model::{
    MarginOrder, MarginOrderQuery, MarginOrderState, OrderResponse, OrderSide, OrderStatus,
    OrderType, SideEffectType, TimeInForce,
};
use chrono::Utc;
use getset::Getters;

use crate::krypto_account::{KryptoAccount, PrecisionData};

const TICK_SIZE_MULTIPLIER: f64 = 2.0;
const TRADE_PERCENTAGE: f64 = 1.0;

#[derive(Debug, Clone)]
pub struct OrderDetails {
    pub ticker: String,
    pub side: OrderSide,
    pub quantity: Option<f64>,
    pub max_time: Option<i64>,
}

#[derive(Getters, Clone)]
#[getset(get = "pub")]
pub struct OrderEvent {
    details: OrderDetails,
    precision: Box<PrecisionData>,
    account: KryptoAccount,
    latest_price: f64,
    current_order_price: Option<f64>,
    current_order_id: Option<u64>,
    mutable_quantity: f64,
}

impl OrderEvent {
    pub async fn new(
        details: OrderDetails,
        mut account: KryptoAccount,
    ) -> Result<Self, Box<dyn Error>> {
        let max_borrow = account.max_borrowable(details.ticker.as_str()).await?;
        let quantity = details.quantity.unwrap_or(max_borrow * TRADE_PERCENTAGE);
        let latest_price = account
            .market
            .get_price(details.ticker.clone())
            .await?
            .price;
        let precision = account.get_precision_data(details.ticker.clone()).await?;
        let mut event = Self {
            details: OrderDetails {
                quantity: Some(quantity),
                ..details
            },
            precision,
            account,
            latest_price,
            current_order_price: None,
            current_order_id: None,
            mutable_quantity: quantity,
        };
        println!("OrderEvent created: {:?}", event);
        event.run().await?;
        Ok(event)
    }

    async fn run(&mut self) -> Result<(), Box<dyn Error>> {
        self.update_latest_price().await?;
        self.place_order_catch().await?;
        loop {
            let order = self.query_order().await?;
            self.update_latest_price().await?;
            match order.status {
                OrderStatus::New => {
                    if self.should_update() {
                        self.cancel_order().await;
                        println!("Order canceled");
                        self.place_order_catch().await?;
                    }
                }
                OrderStatus::Filled => {
                    println!("Order filled");
                    break;
                }
                OrderStatus::PartiallyFilled => {
                    if self.should_update() {
                        self.mutable_quantity -= order.executed_qty;
                        self.cancel_order().await;
                        println!("Order partially filled but canceled");
                        self.place_order_catch().await?;
                    }
                }
                _ => {
                    return Err(Box::new(OrderError::OrderCanceled(format!(
                        "Unknown order status: {:?}. (You may have canceled an order)",
                        order.status
                    ))));
                }
            }
            let wait_time = match self.details.side {
                OrderSide::Buy => 200,
                OrderSide::Sell => 500,
            };
            tokio::time::sleep(tokio::time::Duration::from_millis(wait_time)).await;
            if let Some(max_time) = self.details.max_time {
                if Utc::now().timestamp() - max_time > 0 {
                    return Err(Box::new(OrderError::OrderTimeout));
                }
            }
        }
        Ok(())
    }

    async fn place_order_catch(&mut self) -> Result<(), Box<dyn Error>> {
        let result = self.place_order().await;
        if result.is_err() {
            self.cancel_order().await;
            self.update_latest_price().await?;
            self.place_order().await?;
        }
        Ok(())
    }

    async fn update_latest_price(&mut self) -> Result<(), Box<dyn Error>> {
        let last_price = self.latest_price;
        self.latest_price = self
            .account
            .market
            .get_price(self.details.ticker.clone())
            .await?
            .price;
        if last_price != self.latest_price {
            println!(
                "Updated latest price: {} -> {}",
                last_price, self.latest_price
            );
        }
        Ok(())
    }

    fn get_order(&self) -> Result<MarginOrder, Box<dyn Error>> {
        let client_order_id = format!("{}-{}", self.details.ticker, Utc::now().timestamp());
        let quantity = self.precision.fmt_quantity(self.mutable_quantity)?;
        let price = self.precision.fmt_price(self.get_enter_price())?;
        Ok(MarginOrder {
            symbol: self.details.ticker.clone(),
            side: self.details.side.clone(),
            order_type: OrderType::Limit,
            quantity: Some(quantity),
            quote_order_qty: None,
            price: Some(price),
            stop_price: None,
            new_client_order_id: Some(client_order_id),
            iceberg_qty: None,
            new_order_resp_type: OrderResponse::Full,
            time_in_force: Some(TimeInForce::GTC),
            is_isolated: None,
            side_effect_type: match self.details.side {
                OrderSide::Buy => SideEffectType::MarginBuy,
                OrderSide::Sell => SideEffectType::AutoRepay,
            },
        })
    }

    async fn place_order(&mut self) -> Result<(), Box<dyn Error>> {
        let order = self.get_order()?;
        let order = self.account.margin.new_order(order).await.map_err(|e| {
            Box::new(OrderError::OrderError(format!(
                "Error placing order: {}",
                e
            )))
        })?;
        self.current_order_id = Some(order.order_id);
        self.current_order_price = Some(order.price);
        println!(
            "Placed LIMIT order: {} {:?} {} at ${}",
            order.symbol, order.side, order.orig_qty, order.price
        );
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        Ok(())
    }

    async fn cancel_order(&mut self) {
        let result = self
            .account
            .margin
            .cancel_all_orders(self.details.ticker.clone(), None)
            .await;
        match result {
            Ok(_) => {}
            Err(e) => {
                println!("Error canceling order: {} (This may be an issue with the API and the order may be cancelled).", e);
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    async fn query_order(&mut self) -> Result<MarginOrderState, Box<dyn Error>> {
        let order_id = self.current_order_id.unwrap().to_string();
        let query = MarginOrderQuery {
            symbol: self.details.ticker.clone(),
            order_id: Some(order_id),
            ..Default::default()
        };
        let order = self.account.margin.order(query).await.map_err(|e| {
            Box::new(OrderError::OrderQueryError(format!(
                "Error querying order: {}",
                e
            )))
        })?;
        Ok(order)
    }

    #[inline]
    fn get_difference(&self) -> f64 {
        match self.details.side {
            OrderSide::Buy => -self.precision.tick_size() * TICK_SIZE_MULTIPLIER,
            OrderSide::Sell => self.precision.tick_size() * TICK_SIZE_MULTIPLIER,
        }
    }

    #[inline]
    fn get_enter_price(&self) -> f64 {
        self.latest_price + self.get_difference()
    }

    #[inline]
    fn should_update(&self) -> bool {
        let price_to_order_dif = self.latest_price - self.current_order_price.unwrap();
        let buffer = -self.get_difference();
        match self.details.side {
            OrderSide::Buy => price_to_order_dif > buffer + self.precision.tick_size(),
            OrderSide::Sell => price_to_order_dif < buffer - self.precision.tick_size(),
        }
    }
}

impl core::fmt::Debug for OrderEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OrderEvent")
            .field("details", &self.details)
            .field("precision", &self.precision)
            .field("latest_price", &self.latest_price)
            .field("current_order_price", &self.current_order_price)
            .field("current_order_id", &self.current_order_id)
            .field("mutable_quantity", &self.mutable_quantity)
            .finish()
    }
}

#[derive(thiserror::Error, Debug)]
pub enum OrderError {
    #[error("Order timed out")]
    OrderTimeout,
    #[error("Order error: {0}")]
    OrderError(String),
    #[error("Order status error: {0}")]
    OrderQueryError(String),
    #[error("Order canceled unexpectedly: {0}")]
    OrderCanceled(String),
}
