//! Warren NATS integration — delegates to the `warren-adapter` crate.
//! This module exists for backward compatibility during transition.
mod ingest;

pub use ingest::NatsIngest;
