use thiserror::Error;
use uuid::Uuid;

pub type Result<T> = std::result::Result<T, CortexError>;

#[derive(Debug, Error)]
pub enum CortexError {
    #[error("Storage error: {0}")]
    Storage(Box<redb::Error>),

    #[error("Database error: {0}")]
    Database(#[from] redb::DatabaseError),

    #[error("Table error: {0}")]
    Table(#[from] redb::TableError),

    #[error("Transaction error: {0}")]
    Transaction(Box<redb::TransactionError>),

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

impl From<redb::Error> for CortexError {
    fn from(e: redb::Error) -> Self {
        CortexError::Storage(Box::new(e))
    }
}

impl From<redb::TransactionError> for CortexError {
    fn from(e: redb::TransactionError) -> Self {
        CortexError::Transaction(Box::new(e))
    }
}
