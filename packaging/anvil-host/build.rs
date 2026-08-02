//! Build script del host: copia los dos guests WASM ya compilados a
//! `OUT_DIR` para que `main.rs` los embeba con `include_bytes!`.
//!
//! **No construye los `.wasm` desde aquí** (eso requeriría invocar `cargo`
//! de forma recursiva y pelearse con el lock del build). El orden de build
//! es:
//!
//! ```sh
//! cargo build --target wasm32-wasip2 -p motor -p ejecutor_pasos   # guests
//! cargo build -p anvil-host                                        # host
//! ```
//!
//! Si los artifacts no están, falla con un mensaje claro indicando el
//! comando a correr primero. Busca primero en `debug/` y luego en
//! `release/` (para que el host se pueda compilar en cualquier profile y
//! embeber los guests disponibles).

use std::env;
use std::path::{Path, PathBuf};
use std::process;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    // repo root = crate_dir/../../
    let crate_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let repo_root = crate_dir.ancestors().nth(2).expect("repo root");

    for name in ["anvil-guest.wasm", "ejecutor_pasos.wasm"] {
        let dst = out_dir.join(name);
        let src = ["debug", "release"]
            .iter()
            .map(|p| repo_root.join("target/wasm32-wasip2").join(p).join(name))
            .find(|p| p.exists())
            .unwrap_or_else(|| {
                eprintln!(
                    "Falta el guest '{name}'. Corre primero:\n  \
                     cargo build --target wasm32-wasip2 -p motor -p ejecutor_pasos"
                );
                process::exit(1);
            });
        std::fs::copy(&src, &dst).expect("copiar wasm a OUT_DIR");
        println!("cargo:rerun-if-changed={}", src.display());
    }
    // Rebuild del host si cambian los guests.
    println!("cargo:rerun-if-changed={}", repo_root.join("target/wasm32-wasip2").display());
    let _ = Path::new(&out_dir); // silence unused
}