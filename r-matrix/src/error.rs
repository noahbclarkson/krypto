#[derive(Debug, thiserror::Error)]
pub enum DatasetError {
    #[error("The dataset is empty.")]
    EmptyDataset,
}

#[derive(Debug, thiserror::Error)]
pub enum RMatrixError {
    #[error("The R-matrix has no labels.")]
    LabelsHaveNoLastIndex,
    #[error("The dataset has no features at index {0}.")]
    CantIndexFeatures(usize),
    #[error("Can't find relationship from feature {0} to label {1} at depth {2}.")]
    CantFindRelationship(usize, usize, usize),
    #[error("The wrong number of features was provided. Expected {0}, got {1}.")]
    WrongNumberOfFeatures(usize, usize),
    #[error("The forward depth {0} is too large for the matrix with a depth of {1}.")]
    ForwardDepthTooLarge(usize, usize),
}

pub trait MatrixError {}

impl MatrixError for RMatrixError {}
