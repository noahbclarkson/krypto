use std::error::Error;

#[derive(Debug, thiserror::Error)]
pub enum DatasetError {
    #[error("The dataset is empty.")]
    EmptyDataset,
}

#[derive(Debug, thiserror::Error)]
pub enum RMatrixError {
    #[error("The R-matrix has no labels.")]
    LabelsHaveNoLastIndex,
    #[error("The dataset window can't be indexed at {0}.")]
    CantIndexDatasetWindow(usize),
    #[error("Can't find relationship from feature {0} to label {1} at depth {2}.")]
    CantFindRelationship(usize, usize, usize),
    #[error("The wrong number of features was provided. Expected {0}, got {1}.")]
    WrongNumberOfFeatures(usize, usize),
    #[error("The forward depth {0} is too large for the matrix with a depth of {1}.")]
    ForwardDepthTooLarge(usize, usize),
}

pub trait MatrixError: Error {
    // Method to create a boxed clone of the object
    fn box_clone(&self) -> Box<dyn MatrixError>;
}

impl MatrixError for RMatrixError {
    fn box_clone(&self) -> Box<dyn MatrixError> {
        Box::new(self.clone())
    }
}

impl Clone for Box<dyn MatrixError> {
    fn clone(&self) -> Box<dyn MatrixError> {
        self.box_clone()
    }
}

impl Clone for RMatrixError {
    fn clone(&self) -> Self {
        match self {
            RMatrixError::LabelsHaveNoLastIndex => RMatrixError::LabelsHaveNoLastIndex,
            RMatrixError::CantIndexDatasetWindow(a) => RMatrixError::CantIndexDatasetWindow(*a),
            RMatrixError::CantFindRelationship(a, b, c) => {
                RMatrixError::CantFindRelationship(*a, *b, *c)
            }
            RMatrixError::WrongNumberOfFeatures(a, b) => {
                RMatrixError::WrongNumberOfFeatures(*a, *b)
            }
            RMatrixError::ForwardDepthTooLarge(a, b) => RMatrixError::ForwardDepthTooLarge(*a, *b),
        }
    }
}

impl Error for Box<dyn MatrixError> {}
