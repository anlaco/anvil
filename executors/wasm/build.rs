//! Build script: genera el servicio tonic (server gRPC) desde `paso.proto`
//! (el contrato del motor). El puente es nativo, así que tonic puede usarse
//! sin restricciones (a diferencia de los guests WASM, que usan wasi-grpc).
//! `protoc` va vendido (`protoc-bin-vendored`): el build no depende de
//! protobuf-compiler instalado en el sistema.

fn main() {
    let mut prost = prost_build::Config::new();
    prost.protoc_executable(protoc_bin_vendored::protoc_bin_path().expect("protoc vendido"));
    tonic_build::configure()
        .compile_protos_with_config(
            prost,
            &["../../crates/modelo/paso.proto"],
            &["../../crates/modelo"],
        )
        .expect("compilar paso.proto con tonic_build");
    println!("cargo:rerun-if-changed=../../crates/modelo/paso.proto");
}
