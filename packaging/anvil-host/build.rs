//! Build script del host: copia los dos guests WASM ya compilados y el
//! binario del puente `anvil-puente-wasm` (M5-ext.2, ADR-0015) a `OUT_DIR`
//! para que `main.rs` los embeba con `include_bytes!`.
//!
//! **No construye los `.wasm` ni el puente desde aquí** (eso requeriría
//! invocar `cargo` de forma recursiva y pelearse con el lock del build). El
//! orden de build es:
//!
//! ```sh
//! cargo build --target wasm32-wasip2 -p motor -p ejecutor_pasos   # guests
//! cargo build --manifest-path packaging/anvil-puente-wasm/Cargo.toml  # puente
//! cargo build --manifest-path packaging/anvil-host/Cargo.toml        # host
//! ```
//!
//! Si los artifacts no están, falla con un mensaje claro indicando el
//! comando a correr primero. Busca primero en `debug/` y luego en
//! `release/` (para que el host se pueda compilar en cualquier profile y
//! embeber los artifacts disponibles).

use std::env;
use std::path::PathBuf;
use std::process;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    // repo root = crate_dir/../../
    let crate_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let repo_root = crate_dir.ancestors().nth(2).expect("repo root");

    // (nombre de salida, subdir del target, dir de origen, comando sugerido)
    let piezas: Vec<(&str, &str, &str, &str)> = vec![
        (
            "anvil-guest.wasm",
            "target/wasm32-wasip2",
            "",
            "cargo build --target wasm32-wasip2 -p motor -p ejecutor_pasos",
        ),
        (
            "ejecutor_pasos.wasm",
            "target/wasm32-wasip2",
            "",
            "cargo build --target wasm32-wasip2 -p motor -p ejecutor_pasos",
        ),
        (
            "anvil-puente-wasm",
            "target",
            "packaging/anvil-puente-wasm/",
            "cargo build --manifest-path packaging/anvil-puente-wasm/Cargo.toml",
        ),
    ];

    for (nombre, subdir, dir, comando) in &piezas {
        let dst = out_dir.join(nombre);
        let src = ["debug", "release"]
            .iter()
            .map(|p| repo_root.join(dir).join(subdir).join(p).join(nombre))
            .find(|p| p.exists())
            .unwrap_or_else(|| {
                eprintln!("Falta el artifact '{nombre}'. Corre primero:\n  {comando}");
                process::exit(1);
            });
        std::fs::copy(&src, &dst).expect("copiar artifact a OUT_DIR");
        println!("cargo:rerun-if-changed={}", src.display());
    }
    // Rebuild del host si cambian los artifacts.
    println!("cargo:rerun-if-changed={}", repo_root.join("target/wasm32-wasip2").display());
    println!(
        "cargo:rerun-if-changed={}",
        repo_root.join("packaging/anvil-puente-wasm/target").display()
    );
}
