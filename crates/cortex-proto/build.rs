fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Only regenerate proto code when the `regenerate` feature flag is set.
    // By default, the pre-generated file in src/generated/ is used so that
    // end users don't need protoc installed.
    #[cfg(feature = "regenerate")]
    {
        tonic_build::configure()
            .build_server(true)
            .build_client(true)
            .compile_protos(&["proto/cortex.proto"], &["proto"])?;
    }

    // Tell cargo to rerun if the proto file changes (useful when regenerating)
    println!("cargo:rerun-if-changed=proto/cortex.proto");
    println!("cargo:rerun-if-changed=build.rs");

    Ok(())
}
