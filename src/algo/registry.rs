use std::collections::HashMap;

use crate::algo::strategies::*;
use crate::algo::SignalGenerator;

/// Factory function type for creating strategy instances
pub type StrategyFactory = fn() -> Box<dyn SignalGenerator>;

/// Registry for creating strategy instances by name
/// 
/// This enables runtime strategy instantiation from configuration files,
/// allowing users to specify which strategies to use without recompiling.
pub struct StrategyRegistry {
    factories: HashMap<String, StrategyFactory>,
}

impl StrategyRegistry {
    /// Create a new registry populated with all default strategies
    pub fn new() -> Self {
        let mut registry = Self {
            factories: HashMap::new(),
        };
        registry.register_defaults();
        registry
    }

    /// Register all built-in strategies
    fn register_defaults(&mut self) {
        self.register("dynamic_trend", || Box::new(DynamicTrend::new()));
        self.register("atr_breakout", || Box::new(AtrBreakout::new()));
        self.register("rsi_mean_reversion", || Box::new(RsiMeanReversion::new()));
        self.register("bollinger_reversion", || Box::new(BollingerReversion::new()));
        self.register("volatility_squeeze", || Box::new(VolatilitySqueeze::new()));
        self.register("macd_trend", || Box::new(MacdTrend::new()));
        self.register("obv_trend", || Box::new(ObvTrend::new()));
        self.register("price_momentum", || Box::new(PriceMomentum::new()));
        self.register(
            "adaptive_ma_crossover",
            || Box::new(AdaptiveMaCrossover::new()),
        );
        self.register(
            "relative_strength",
            || Box::new(RelativeStrengthStrat::new()),
        );
        self.register("lead_lag", || Box::new(LeadLagStrategy::new()));
    }

    /// Register a custom strategy factory
    /// 
    /// # Arguments
    /// * `name` - Unique identifier for the strategy
    /// * `factory` - Factory function that creates a new instance
    pub fn register(&mut self, name: &str, factory: StrategyFactory) {
        self.factories.insert(name.to_string(), factory);
    }

    /// Create a strategy instance by name
    /// 
    /// # Arguments
    /// * `name` - The registered strategy name
    /// 
    /// # Returns
    /// * `Some(Box<dyn SignalGenerator>)` if the strategy exists
    /// * `None` if no strategy is registered with that name
    pub fn create(&self, name: &str) -> Option<Box<dyn SignalGenerator>> {
        self.factories.get(name).map(|f| f())
    }

    /// Get a list of all registered strategy names
    pub fn available_strategies(&self) -> Vec<&str> {
        self.factories.keys().map(|s| s.as_str()).collect()
    }

    /// Check if a strategy is registered
    pub fn contains(&self, name: &str) -> bool {
        self.factories.contains_key(name)
    }

    /// Get the number of registered strategies
    pub fn len(&self) -> usize {
        self.factories.len()
    }

    /// Check if the registry is empty
    pub fn is_empty(&self) -> bool {
        self.factories.is_empty()
    }
}

impl Default for StrategyRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creates_all_registered_strategies() {
        let registry = StrategyRegistry::new();
        let strategies = registry.available_strategies();

        // Verify we have all expected strategies
        assert!(!strategies.is_empty(), "Registry should have strategies");

        // Test that each registered strategy can be created
        for name in &strategies {
            let strategy = registry.create(name);
            assert!(
                strategy.is_some(),
                "Should be able to create strategy '{}'",
                name
            );

            let strategy = strategy.unwrap();
            // Verify the strategy implements SignalGenerator by calling name()
            let strategy_name = strategy.name();
            assert!(
                !strategy_name.is_empty(),
                "Strategy '{}' should have a non-empty name",
                name
            );
        }
    }

    #[test]
    fn test_unknown_strategy_returns_none() {
        let registry = StrategyRegistry::new();

        let result = registry.create("nonexistent_strategy");
        assert!(
            result.is_none(),
            "Unknown strategy should return None"
        );

        let result = registry.create("");
        assert!(
            result.is_none(),
            "Empty string should return None"
        );

        let result = registry.create("random_name_12345");
        assert!(
            result.is_none(),
            "Random name should return None"
        );
    }

    #[test]
    fn test_strategy_names_are_accessible() {
        let registry = StrategyRegistry::new();
        let names = registry.available_strategies();

        // Verify expected strategy names are present
        let expected_names = vec![
            "dynamic_trend",
            "atr_breakout",
            "rsi_mean_reversion",
            "bollinger_reversion",
            "volatility_squeeze",
            "macd_trend",
            "obv_trend",
            "price_momentum",
            "adaptive_ma_crossover",
            "relative_strength",
            "lead_lag",
        ];

        for expected in &expected_names {
            assert!(
                names.contains(expected),
                "Registry should contain '{}'",
                expected
            );
        }

        // Verify count matches
        assert_eq!(
            names.len(),
            expected_names.len(),
            "Should have exactly {} strategies",
            expected_names.len()
        );
    }

    #[test]
    fn test_register_custom_strategy() {
        let mut registry = StrategyRegistry::new();
        let initial_count = registry.len();

        // Register a duplicate (should overwrite)
        registry.register("dynamic_trend", || Box::new(DynamicTrend::new()));
        assert_eq!(
            registry.len(),
            initial_count,
            "Re-registering should not increase count"
        );

        // Verify contains works
        assert!(registry.contains("dynamic_trend"));
        assert!(!registry.contains("unknown"));
    }

    #[test]
    fn test_registry_len_and_empty() {
        let registry = StrategyRegistry::new();
        assert!(!registry.is_empty());
        assert_eq!(registry.len(), 11);
    }

    #[test]
    fn test_create_returns_valid_signal_generator() {
        let registry = StrategyRegistry::new();

        // Create a DynamicTrend and verify it's a valid SignalGenerator
        let strategy = registry.create("dynamic_trend").expect("Should create dynamic_trend");
        assert_eq!(strategy.name(), "Dynamic_Trend");

        // Create a MacdTrend and verify
        let strategy = registry.create("macd_trend").expect("Should create macd_trend");
        assert_eq!(strategy.name(), "MACD_Trend");

        // Create an RsiMeanReversion and verify
        let strategy = registry.create("rsi_mean_reversion").expect("Should create rsi_mean_reversion");
        assert_eq!(strategy.name(), "RSI_Reversion");
    }
}
