use thiserror::Error;
use uuid::Uuid;

pub type Result<T> = std::result::Result<T, CortexError>;

#[derive(Debug, Error)]
pub enum CortexError {
    #[error("Storage error: {0}")]
    Storage(#[from] redb::Error),

    #[error("Database error: {0}")]
    Database(#[from] redb::DatabaseError),

    #[error("Table error: {0}")]
    Table(#[from] redb::TableError),

    #[error("Transaction error: {0}")]
    Transaction(#[from] redb::TransactionError),

    #[error("Commit error: {0}")]
    Commit(#[from] redb::CommitError),

    #[error("Storage operation error: {0}")]
    StorageOperation(#[from] redb::StorageError),

    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),

    #[error("Node not found: {0}")]
    NodeNotFound(Uuid),

    #[error("Edge not found: {0}")]
    EdgeNotFound(Uuid),

    #[error("Invalid edge: {reason}")]
    InvalidEdge { reason: String },

    #[error("Duplicate node: {0}")]
    DuplicateNode(Uuid),

    #[error("Duplicate edge: from={from}, to={to}, relation={relation}")]
    DuplicateEdge {
        from: Uuid,
        to: Uuid,
        relation: String,
    },

    #[error("Validation error: {0}")]
    Validation(String),
}
