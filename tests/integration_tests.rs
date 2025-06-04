use krypto::config::KryptoConfig;
use krypto::data::dataset::overall_dataset::Dataset;
use krypto::error::KryptoError;
use std::path::Path;

// Helper to load config for tests
async fn load_test_config() -> Result<KryptoConfig, KryptoError> {
    // Create a temporary default config if needed, or use a specific test config file
    let config_path = Path::new("test_config.yml");
    if !config_path.exists() {
        let default_config = KryptoConfig::default();
        let yaml = serde_yaml::to_string(&default_config).unwrap();
        tokio::fs::write(config_path, yaml).await.unwrap();
    }
    KryptoConfig::read_config(Some(config_path)).await
}

#[tokio::test]
#[ignore] // Ignore by default as it might require API keys or network access
async fn test_load_full_dataset() {
    // This test requires network access and potentially API keys if cache is empty
    // TODO: Implement mocking for Binance API calls

    let config = load_test_config()
        .await
        .expect("Failed to load test config");

    // Ensure cache is disabled or cleared for a real fetch test?
    // config.cache_enabled = false;

    let dataset_result = Dataset::load(&config).await;

    assert!(
        dataset_result.is_ok(),
        "Dataset loading failed: {:?}",
        dataset_result.err()
    );

    let dataset = dataset_result.unwrap();
    assert!(!dataset.is_empty(), "Loaded dataset is empty");
    assert_eq!(
        dataset.len(),
        config.intervals.len(),
        "Dataset does not contain all configured intervals"
    );

    // Add more assertions about dataset shape, content, etc.
    let shape = dataset.shape();
    println!("Loaded dataset shape: {:?}", shape);
    for interval in &config.intervals {
        assert!(
            dataset.get(interval).is_some(),
            "Interval {} missing from dataset",
            interval
        );
        let interval_data = dataset.get(interval).unwrap();
        assert_eq!(
            interval_data.len(),
            config.symbols.len(),
            "Interval {} does not contain all configured symbols",
            interval
        );
        // Check if symbol data is loaded
        for symbol in &config.symbols {
            assert!(
                interval_data.get(symbol).is_some(),
                "Symbol {} missing from interval {}",
                symbol,
                interval
            );
            let symbol_data = interval_data.get(symbol).unwrap();
            assert!(
                !symbol_data.is_empty(),
                "Symbol data for {} on interval {} is empty",
                symbol,
                interval
            );
        }
    }
}

#[tokio::test]
#[ignore] // Needs dataset loading and potentially complex setup
async fn test_backtest_flow() {
    // 1. Load config
    // 2. Load dataset
    // 3. Choose settings
    // 4. Load algorithm (which runs walk-forward)
    // 5. Assert on algorithm.result properties (e.g., check if metrics are reasonable)
    // 6. Run backtest_on_all_seen_data
    // 7. Assert on the full backtest result
    todo!("Implement backtest integration test");
}

#[tokio::test]
#[ignore] // Requires mocking or significant setup
async fn test_trade_logic_mocked() {
    // 1. Setup Mock Binance API (using wiremock or similar)
    // 2. Configure KryptoAccount to use the mock server
    // 3. Simulate market data / predictions
    // 4. Run parts of the trade loop logic
    // 5. Assert that the correct API calls (make_trade) were made to the mock server
    todo!("Implement mocked trading integration test");
}

// Clean up test config file
// This requires a way to run code after all tests in the module.
// Standard Rust test runner doesn't have direct support for module teardown.
// Could use a custom test harness or just manually delete the file.
// Alternatively, use tempfile crate to create temporary config files.
