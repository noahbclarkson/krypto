#[derive(Debug, thiserror::Error, PartialEq, Eq, Clone)]
/// An error type for the RMatrix crate.
pub enum RError {
    #[error("The RMatrix dataset must have at least one target entry.")]
    NoTargetEntryError,
    #[error("The RMatrix dataset must have at least one record entry.")]
    NoRecordEntryError,
    #[error("The RMatrix dataset must have only one target entry")]
    MultipleTargetEntriesError,
    #[error("The number of RMatrix dataset records must match the number of relationships.")]
    RelationshipRecordCountMismatchError,
    #[error("The RMatrix dataset record index is out of bounds.")]
    RecordIndexOutOfBoundsError,
}
