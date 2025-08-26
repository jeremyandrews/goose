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

    // Use the correct API for tonic-build 0.14 - function moved to prost_build
    prost_build::compile_protos(&["proto/gaggle.proto"], &["proto/"])
        .unwrap_or_else(|e| panic!("Failed to compile protos: {}", e));
}
