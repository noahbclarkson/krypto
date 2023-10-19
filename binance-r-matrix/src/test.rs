#[cfg(test)]
mod tests {
    use r_matrix::{
        data::{RData, RDataEntry},
        math::NormalizationFunctionType,
        matricies::SimpleRMatrix,
    };

    use crate::{BinanceDataId, BinanceDataType};

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
        let mut r_matrix: SimpleRMatrix<BinanceDataId> = SimpleRMatrix::new(1, function);
        r_matrix.calculate_relationships(&result).await;
        let result = r_matrix.predict_stable(&result, 1).await;
        let result = result.ok();
        match result {
            Some(_) => (),
            _ => panic!("Expected the prediction to be valid"),
        };
    }
}
