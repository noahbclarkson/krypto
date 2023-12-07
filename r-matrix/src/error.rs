#[derive(Debug, thiserror::Error)]
pub enum DatasetError {
    #[error("The dataset is empty.")]
    EmptyDataset,
    #[error("CSV error: {0}")]
    CsvError(#[from] csv::Error),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum RMatrixError {
    #[error("Can't find relationship from feature {0} to label {1} at depth {2}.")]
    CantFindRelationship(usize, usize, usize),
    #[error("Invalid dataset size.")]
    InvalidDatasetSize,
    #[error("Invalid data point index {0}.")]
    InvalidDataPointIndex(usize),
}