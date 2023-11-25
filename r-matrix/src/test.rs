#[cfg(test)]
mod tests {
    use crate::data::{RData, RDataEntry, RMatrixId};

    #[derive(Debug, PartialEq, Eq, Clone)]
    struct TestIdentity {
        id: String,
        target: bool,
    }

    impl TestIdentity {
        fn new(id: String, target: bool) -> Self {
            Self { id, target }
        }
    }

    impl RMatrixId for TestIdentity {
        fn get_id(&self) -> &str {
            &self.id
        }

        fn is_target(&self) -> bool {
            self.target
        }
    }

    #[test]
    fn test_new_rmatrix_data_valid() {
        let data = vec![
            RDataEntry::new(TestIdentity::new("1".to_string(), true), vec![1.0, 2.0]),
            RDataEntry::new(TestIdentity::new("2".to_string(), false), vec![2.0, 3.0]),
        ];

        let result = RData::<TestIdentity>::new(data);
        assert!(result.is_ok());

        let rmatrix_data = result.unwrap();
        assert_eq!(rmatrix_data.records().len(), 1);
        assert!(rmatrix_data.target().id().is_target());
    }
}
