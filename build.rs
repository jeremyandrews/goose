fn main() {
    // Only compile protobuf files if the gaggle feature is enabled
    #[cfg(feature = "gaggle")]
    compile_protos();

    // Existing build logic
    let version = rustc_version::version().unwrap();
    if version.major == 1 && version.minor < 64 {
        println!("cargo:rustc-cfg=rust_pre_164");
    }
}

#[cfg(feature = "gaggle")]
fn compile_protos() {
    println!("cargo:rerun-if-changed=proto/gaggle.proto");

    // Use tonic_prost_build - the correct API for tonic 0.14
    tonic_prost_build::configure()
        .compile_protos(&["proto/gaggle.proto"], &["proto"])
        .unwrap_or_else(|e| panic!("Failed to compile protos: {}", e));
}
