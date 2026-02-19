mod service;
mod conversions;

pub use service::CortexServiceImpl;
pub use conversions::*;

use tonic::{Request, Status};

/// Helper to extract metadata from gRPC requests
pub fn get_metadata<T>(request: &Request<T>, key: &str) -> Option<String> {
    request
        .metadata()
        .get(key)
        .and_then(|v| v.to_str().ok())
        .map(String::from)
}

/// Convert anyhow::Error to tonic::Status
pub fn to_status(err: anyhow::Error) -> Status {
    Status::internal(err.to_string())
}
