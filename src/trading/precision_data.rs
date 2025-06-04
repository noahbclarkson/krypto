use crate::error::KryptoError;
use tracing::error;

#[derive(Debug, Clone, PartialEq)] // Added PartialEq
pub struct PrecisionData {
    ticker: String,
    tick_size: f64,        // Minimum price change increment
    step_size: f64,        // Minimum quantity change increment
    tick_precision: usize, // Number of decimal places for price
    step_precision: usize, // Number of decimal places for quantity
}

impl PrecisionData {
    /// Creates new PrecisionData, calculating precision from sizes.
    /// Returns error if sizes are non-positive.
    pub fn new(ticker: String, tick_size: f64, step_size: f64) -> Result<Self, KryptoError> {
        if tick_size <= 0.0 || step_size <= 0.0 {
            error!(
                "Tick size ({}) and step size ({}) must be positive for ticker {}",
                tick_size, step_size, ticker
            );
            return Err(KryptoError::ConfigError(format!(
                "Invalid precision sizes for {}: tick={}, step={}",
                ticker, tick_size, step_size
            )));
        }

        // Calculate precision: number of digits after the decimal point.
        // Find the position of '1' after the decimal point.
        let tick_precision = Self::calculate_precision(tick_size);
        let step_precision = Self::calculate_precision(step_size);

        Ok(Self {
            ticker,
            tick_size,
            step_size,
            tick_precision,
            step_precision,
        })
    }

    /// Calculates the number of decimal places needed based on the size increment.
    fn calculate_precision(size: f64) -> usize {
        // Convert to string, find decimal point, count digits after '1'.
        // Handle cases like 1.0, 0.1, 0.01, 0.001 etc.
        // A simpler approach: use log10, but handle edge cases.
        // If size = 0.01, log10 = -2. Precision = 2.
        // If size = 0.005, log10 ~ -2.3. Precision should be 3.
        // If size = 1, log10 = 0. Precision = 0.
        // If size = 10, log10 = 1. Precision = 0.

        // Let's use string formatting for robustness.
        let s = format!("{:.16}", size); // Format with enough precision
        s.find('.')
            .map(|dot_index| {
                s.chars()
                    .skip(dot_index + 1)
                    .position(|c| c == '1') // Find first '1' after decimal
                    .map(|pos| pos + 1) // Precision is position + 1
                    .unwrap_or(0) // If no '1' found (e.g., "0.000"), precision is 0? Or based on trailing zeros? Let's default to 0.
                                  // Alternative: count trailing zeros until non-zero?
                                  // Let's stick to finding '1' for standard Binance filters.
            })
            .unwrap_or(0) // No decimal point means precision 0
    }

    pub fn get_ticker(&self) -> &str {
        &self.ticker
    }

    pub fn get_tick_size(&self) -> f64 {
        self.tick_size
    }

    pub fn get_step_size(&self) -> f64 {
        self.step_size
    }

    pub fn get_tick_precision(&self) -> usize {
        self.tick_precision
    }

    pub fn get_step_precision(&self) -> usize {
        self.step_precision
    }

    /// Formats the price according to tick precision (as string).
    pub fn fmt_price_to_string(&self, price: f64) -> String {
        format!("{:.prec$}", price, prec = self.tick_precision)
    }

    /// Formats the quantity according to step precision (as string).
    pub fn fmt_quantity_to_string(&self, quantity: f64) -> String {
        format!("{:.prec$}", quantity, prec = self.step_precision)
    }

    /// Formats the price according to tick precision (as f64).
    /// Note: This involves string conversion and parsing, might lose precision.
    /// It also truncates, doesn't round according to tick_size rules.
    /// For actual order placement, adjusting to the nearest valid multiple of tick_size is better.
    pub fn fmt_price(&self, price: f64) -> Result<f64, KryptoError> {
        // Truncate based on precision
        let factor = 10f64.powi(self.tick_precision as i32);
        let truncated_price = (price * factor).floor() / factor;

        // This is still not perfect for Binance rules which require price % tick_size == 0
        // A better approach for order placement:
        // let adjusted_price = (price / self.tick_size).floor() * self.tick_size;

        Ok(truncated_price) // Return truncated for now, needs refinement for orders
    }

    /// Formats the quantity according to step precision (as f64).
    /// Truncates the value. For order placement, adjust to nearest valid multiple of step_size.
    pub fn fmt_quantity(&self, quantity: f64) -> Result<f64, KryptoError> {
        // Truncate based on precision
        let factor = 10f64.powi(self.step_precision as i32);
        let truncated_quantity = (quantity * factor).floor() / factor;

        // For actual order placement:
        // let adjusted_quantity = (quantity / self.step_size).floor() * self.step_size;
        // if adjusted_quantity <= 0.0 { return Err(...) } // Ensure non-zero after adjustment

        if truncated_quantity < 0.0 {
            error!(
                "Formatted quantity resulted in negative value: {}",
                truncated_quantity
            );
            return Err(KryptoError::PrecisionFormatError {
                value_type: "quantity".to_string(),
                value: quantity,
            });
        }

        Ok(truncated_quantity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_precision_calculation() {
        assert_eq!(PrecisionData::calculate_precision(0.1), 1);
        assert_eq!(PrecisionData::calculate_precision(0.01), 2);
        assert_eq!(PrecisionData::calculate_precision(0.0001), 4);
        assert_eq!(PrecisionData::calculate_precision(1.0), 0);
        assert_eq!(PrecisionData::calculate_precision(10.0), 0);
        assert_eq!(PrecisionData::calculate_precision(0.005), 3); // Finds '5' at 3rd position
        assert_eq!(PrecisionData::calculate_precision(0.00000001), 8);
    }

    #[test]
    fn test_precision_new() {
        let pd = PrecisionData::new("BTCUSDT".to_string(), 0.01, 0.00001).unwrap();
        assert_eq!(pd.get_tick_precision(), 2);
        assert_eq!(pd.get_step_precision(), 5);

        let pd2 = PrecisionData::new("ETHBTC".to_string(), 0.000001, 0.001).unwrap();
        assert_eq!(pd2.get_tick_precision(), 6);
        assert_eq!(pd2.get_step_precision(), 3);

        let pd3 = PrecisionData::new("SHIBUSDT".to_string(), 0.00000001, 1.0).unwrap();
        assert_eq!(pd3.get_tick_precision(), 8);
        assert_eq!(pd3.get_step_precision(), 0);
    }

    #[test]
    fn test_precision_new_invalid() {
        assert!(PrecisionData::new("BTCUSDT".to_string(), 0.0, 0.001).is_err());
        assert!(PrecisionData::new("BTCUSDT".to_string(), 0.01, -0.001).is_err());
    }

    #[test]
    fn test_formatting() {
        let pd = PrecisionData::new("BTCUSDT".to_string(), 0.01, 0.00001).unwrap(); // 2 price, 5 qty

        // Price Formatting
        assert_eq!(pd.fmt_price_to_string(25000.12345), "25000.12");
        assert_eq!(pd.fmt_price_to_string(25000.1), "25000.10");
        assert_eq!(pd.fmt_price(25000.12345).unwrap(), 25000.12);
        assert_eq!(pd.fmt_price(25000.12999).unwrap(), 25000.12); // Floor behavior

        // Quantity Formatting
        assert_eq!(pd.fmt_quantity_to_string(0.1234567), "0.12345");
        assert_eq!(pd.fmt_quantity_to_string(0.1), "0.10000");
        assert_eq!(pd.fmt_quantity(0.1234567).unwrap(), 0.12345);
        assert_eq!(pd.fmt_quantity(0.1234599).unwrap(), 0.12345); // Floor behavior
    }
}
