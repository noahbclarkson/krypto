use proptest::{prop_assert, prop_assert_eq, test_runner::Config};

use crate::common::arb_dataset;

proptest::proptest! {
    #![proptest_config(Config::with_cases(100))]

    // Test 1: Dataset Integrity
    #[test]
    fn test_dataset_integrity(data in arb_dataset()) {
        // Verify dataset length
        let expected_length = data.len();
        let actual_length = data.iter().count();
        prop_assert_eq!(expected_length, actual_length);

        // Verify integrity of each DataPoint
        for dp in data.iter() {
            prop_assert!(!dp.features().is_empty());
            prop_assert!(!dp.labels().is_empty());
        }
    }

    // Test 2: Sorting by Time
    #[test]
    fn test_sorting_by_time(mut data in arb_dataset()) {
        data.sort_by_time();

        let mut is_sorted = true;
        for window in data.windowed_iter(2) {
            if window.len() > 1 && window[0].time() > window[1].time() {
                is_sorted = false;
                break;
            }
        }
        prop_assert!(is_sorted);
    }

    // Test 3: Windowed Iteration
    #[test]
    fn test_windowed_iteration(data in arb_dataset(), window_size in 1usize..10usize) {
        let iterator = data.windowed_iter(window_size);
        for window in iterator {
            prop_assert_eq!(window.len(), window_size);
        }
    }

    // Test 4: Dataset Length Consistency
    #[test]
    fn test_dataset_length_consistency(data in arb_dataset()) {
        let feature_names_len = data.feature_names().len();
        let label_names_len = data.label_names().len();

        for dp in data.iter() {
            prop_assert_eq!(dp.features().len(), feature_names_len, "Features length mismatch");
            prop_assert_eq!(dp.labels().len(), label_names_len, "Labels length mismatch");
        }
    }

}
