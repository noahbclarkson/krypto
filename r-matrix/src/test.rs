#[cfg(test)]
mod tests {
    use crate::{errors::RError, RData, RDataEntry, RMatrixId};

    #[derive(Debug, PartialEq, Eq, Clone)]
    struct TestIdentity {
        id: String,
        target: bool,
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
    fn test_new_rmatrix_data_no_target() {
        let data = vec![
            RDataEntry {
                id: TestIdentity {
                    id: "1".to_string(),
                    target: false,
                },
                data: vec![1.0, 2.0],
            },
            RDataEntry {
                id: TestIdentity {
                    id: "2".to_string(),
                    target: false,
                },
                data: vec![2.0, 3.0],
            },
        ];

        let result = RData::<TestIdentity>::new(data);
        let result = result.err();
        match result {
            Some(error) => assert_eq!(error, RError::NoTargetEntryError),
            _ => panic!("Expected NoTargetEntryError"),
        }
    }

    #[test]
    fn test_new_rmatrix_data_multiple_targets() {
        let data = vec![
            RDataEntry {
                id: TestIdentity {
                    id: "1".to_string(),
                    target: true,
                },
                data: vec![1.0, 2.0],
            },
            RDataEntry {
                id: TestIdentity {
                    id: "2".to_string(),
                    target: true,
                },
                data: vec![2.0, 3.0],
            },
            RDataEntry {
                id: TestIdentity {
                    id: "3".to_string(),
                    target: false,
                },
                data: vec![3.0, 4.0],
            },
        ];

        let result = RData::<TestIdentity>::new(data);
        let result = result.err();
        match result {
            Some(error) => assert_eq!(error, RError::MultipleTargetEntriesError),
            _ => panic!("Expected TooManyTargetEntriesError"),
        }
    }

    #[test]
    fn test_new_rmatrix_data_no_records() {
        let data = vec![RDataEntry {
            id: TestIdentity {
                id: "1".to_string(),
                target: true,
            },
            data: vec![1.0, 2.0],
        }];

        let result = RData::<TestIdentity>::new(data);
        let result = result.err();
        match result {
            Some(error) => assert_eq!(error, RError::NoRecordEntryError),
            _ => panic!("Expected NoRecordEntryError"),
        }
    }

    #[test]
    fn test_new_rmatrix_data_valid() {
        let data = vec![
            RDataEntry {
                id: TestIdentity {
                    id: "1".to_string(),
                    target: false,
                },
                data: vec![1.0, 2.0],
            },
            RDataEntry {
                id: TestIdentity {
                    id: "2".to_string(),
                    target: true,
                },
                data: vec![2.0, 3.0],
            },
        ];

        let result = RData::<TestIdentity>::new(data);
        assert!(result.is_ok());

        let rmatrix_data = result.unwrap();
        assert_eq!(rmatrix_data.records().len(), 1);
        assert_eq!(rmatrix_data.target().id().is_target(), true);
    }
}
