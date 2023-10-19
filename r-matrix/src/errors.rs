#[derive(Debug, thiserror::Error)]
/// An error type for the RMatrix crate.
pub enum RError {
    #[error("The RMatrix dataset must have at least one target entry.")]
    NoTargetEntryError,
    #[error("The RMatrix dataset must have at least one record entry.")]
    NoRecordEntryError,
    #[error("The RMatrix dataset must have only one target entry (found {0}).")]
    MultipleTargetEntriesError(usize),
    #[error("The number of RMatrix dataset records must match the number of relationships (found {0} records and {1} relationships).")]
    RelationshipRecordCountMismatchError(usize, usize),
    #[error("The RMatrix dataset record index is out of bounds (found {index} but the length is {length}).")]
    RecordIndexOutOfBoundsError {
        index: usize,
        length: usize,
    },
    #[error("The RMatrix relationship index is out of bounds (found {index} but the length is {length}).")]
    RelationshipIndexOutOfBoundsError {
        index: usize,
        length: usize,
    },
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}
