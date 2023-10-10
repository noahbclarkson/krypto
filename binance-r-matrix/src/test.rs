#[cfg(test)]
mod tests {
    use r_matrix::{errors::RError, math::NormalizationFunctionType, RData, RDataEntry, RMatrix};

    use crate::{BinanceDataId, BinanceDataType};

    #[test]
    fn test_r_matrix_creation_no_entry() {
        let data = vec![
            RDataEntry::new(
                BinanceDataId::new(BinanceDataType::PercentageChange),
                vec![1.0, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0],
            ),
            RDataEntry::new(
                BinanceDataId::new(BinanceDataType::PercentageChange),
                vec![1.0, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0],
            ),
        ];

        let result = RData::<BinanceDataId>::new(data);
        let result = result.err();
        match result {
            Some(error) => assert_eq!(error, RError::NoTargetEntryError),
            _ => panic!("Expected NoTargetEntryError"),
        }
    }

    #[test]
    fn test_r_matrix_creation_multiple_entries() {
        let data = vec![
            RDataEntry::new(
                BinanceDataId::new(BinanceDataType::PercentageChange),
                vec![1.0, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0],
            ),
            RDataEntry::new(
                BinanceDataId::new(BinanceDataType::PercentageChangeReal),
                vec![1.0, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0],
            ),
            RDataEntry::new(
                BinanceDataId::new(BinanceDataType::PercentageChangeReal),
                vec![1.0, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0],
            ),
        ];

        let result = RData::<BinanceDataId>::new(data);
        let result = result.err();
        match result {
            Some(error) => assert_eq!(error, RError::MultipleTargetEntriesError(2)),
            _ => panic!("Expected MultipleTargetEntriesError"),
        }
    }

    #[tokio::test]
    async fn valid_prediction() {
        let data = vec![
            RDataEntry::new(
                BinanceDataId::new(BinanceDataType::PercentageChange),
                vec![1.0, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0],
            ),
            RDataEntry::new(
                BinanceDataId::new(BinanceDataType::PercentageChangeReal),
                vec![1.0, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0],
            ),
        ];

        let result = RData::<BinanceDataId>::new(data);
        let result = result.ok();
        let result = match result {
            Some(res) => res,
            _ => panic!("Expected the RData to be created"),
        };
        let function = NormalizationFunctionType::default();
        let mut r_matrix: RMatrix<BinanceDataId> = RMatrix::new(1, function);
        r_matrix.calculate_relationships(&result).await;
        let result = r_matrix.predict_stable(&result, 1).await;
        let result = result.ok();
        match result {
            Some(_) => (),
            _ => panic!("Expected the prediction to be valid"),
        };
    }
}
