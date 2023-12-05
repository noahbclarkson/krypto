use proptest::{
    arbitrary::any, prelude::prop, prop_assert, prop_assert_eq, strategy::Strategy,
    test_runner::Config,
};
use r_matrix::{dataset::Dataset, matrix::Matrix as _, r_matrix::matrix::RMatrixBuilder, error::RMatrixError};

pub fn arb_dataset() -> impl Strategy<Value = Dataset> {
    let feature_count = any::<usize>().prop_map(|x| x % 10 + 1); // Ensures at least 1 feature, up to 10
    let label_count = any::<usize>().prop_map(|x| x % 10 + 1); // Ensures at least 1 label, up to 10

    (feature_count, label_count).prop_flat_map(|(fc, lc)| {
        let feature_vec = prop::collection::vec(any::<f64>(), fc);
        let label_vec = prop::collection::vec(any::<f64>(), lc);

        prop::collection::vec((any::<usize>(), feature_vec, label_vec), 0..50).prop_map(
            move |data| {
                let feature_names = (1..=fc)
                    .map(|i| format!("feature_{}", i))
                    .collect::<Vec<_>>();
                let label_names = (1..=lc).map(|i| format!("label_{}", i)).collect::<Vec<_>>();
                let mut builder = Dataset::builder();
                for (time, features, labels) in data.iter() {
                    builder.add_data_point(*time, features.clone(), labels.clone());
                }
                builder
                    .set_feature_names(feature_names)
                    .set_label_names(label_names);
                builder.build().unwrap()
            },
        )
    })
}

proptest::proptest! {
    #![proptest_config(Config::with_cases(10))]

    // Test 1: R-Matrix Integrity
    #[test]
    fn test_r_matrix_integrity(data in arb_dataset()) {
        let mut r_matrix = RMatrixBuilder::default();
        r_matrix.depth(2);
        r_matrix.dataset(Box::new(data.clone()));
        let mut r_matrix = r_matrix.build().unwrap();
        let result = r_matrix.train(&data);
        if let Err(e) = result.clone() {
            println!("Error: {}", e);
        }
        prop_assert!(result.is_ok());


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

    fn test_r_matrix_prediction(data in arb_dataset()) {
        let mut r_matrix = RMatrixBuilder::default();
        r_matrix.depth(2);
        r_matrix.dataset(Box::new(data.clone()));
        let mut r_matrix = r_matrix.build().unwrap();
        let result = r_matrix.train(&data);
        if let Err(e) = result.clone() {
            println!("Error: {}", e);
        }
        prop_assert!(result.is_ok());
        let window = data.windowed_iter(2).collect::<Vec<_>>();

        let features = window[0].iter().map(|dp| dp.features()).collect::<Vec<_>>();
        let result = r_matrix.predict(&features, 0);
        if let Err(e) = result.clone() {
            println!("Error: {}", e);
        }
        prop_assert!(result.is_ok());
    }
}
