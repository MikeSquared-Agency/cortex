/// Generated protobuf code
pub mod cortex {
    pub mod v1 {
        // When the `regenerate` feature is enabled, use the build-time generated file.
        // Otherwise, use the pre-generated file committed to the repository.
        #[cfg(feature = "regenerate")]
        tonic::include_proto!("cortex.v1");

        #[cfg(not(feature = "regenerate"))]
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/generated/cortex.v1.rs"
        ));
    }
}

pub use cortex::v1::*;

// Re-export prost_types so generated code can find it
pub use prost_types;
