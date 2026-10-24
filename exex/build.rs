fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto/nodestream.proto");
    tonic_build::configure()
        .build_server(true)
        .build_client(false)
        .compile(&["proto/nodestream.proto"], &["proto"])?;

    vergen::Emitter::default()
        .add_instructions(&vergen::BuildBuilder::default().build()?)
        .expect("vergen build")
        .add_instructions(&vergen::CargoBuilder::default().build()?)
        .expect("vergen cargo")
        .emit()
        .expect("vergen emit");

    Ok(())
}
