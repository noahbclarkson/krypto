use proptest::test_runner::Config;
use r_matrix::r_matrix::{cmaes::RMatrixCMAESSettingsBuilder, matrix::RMatrixBuilder};

use crate::common::arb_dataset;

proptest::proptest! {
    #![proptest_config(Config::with_cases(100))]

    #[test]
    fn r_matrix_creation(_ in arb_dataset()) {
        let r_matrix = RMatrixBuilder::default().build();
        assert!(r_matrix.is_ok());
    }

    #[test]
    fn r_matrix_train(data in arb_dataset()) {
        if data.is_empty() {
            return Ok(());
        }
        let mut r_matrix = RMatrixBuilder::default().build().unwrap();
        let data = Box::new(data);
        let result = r_matrix.train(&data);
        assert!(result.is_ok());
    }

    #[test]
    fn r_matrix_optimize(data in arb_dataset()) {
        if data.len() < 2 {
            return Ok(());
        }
        let cmaes_settings = RMatrixCMAESSettingsBuilder::default().build().unwrap();
        let mut r_matrix = RMatrixBuilder::default().build().unwrap();
        let data = Box::new(data);
        r_matrix.train(&data).unwrap();
        r_matrix.optimize(&data, cmaes_settings);
    }
}
