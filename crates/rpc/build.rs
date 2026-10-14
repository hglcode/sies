fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_prost_build::configure()
        .build_server(true)
        .compile_protos(
            &[
                "proto/v1/common.proto",
                "proto/v1/node_verifier.proto",
                "proto/v1/peer.proto",
            ],
            &["proto"],
        )?;
    Ok(())
}
