//! The host's build script: copies the two already-compiled WASM guests and
//! the `anvil-puente-wasm` bridge binary (M5-ext.2, ADR-0015) into `OUT_DIR`
//! so that `main.rs` can embed them with `include_bytes!`.
//!
//! **It does not build the `.wasm` files nor the bridge from here** (that
//! would require invoking `cargo` recursively and fighting it over the build
//! lock). The build order is:
//!
//! ```sh
//! cargo build --target wasm32-wasip2 -p motor -p ejecutor_pasos   # guests
//! cargo build --manifest-path executors/wasm/Cargo.toml              # bridge
//! cargo build --manifest-path packaging/anvil-host/Cargo.toml        # host
//! ```
//!
//! (`make build` / `make release` at the repo root do it in order.)
//!
//! If the artifacts are missing, it fails with a clear message naming the
//! command to run first. It looks **first in the profile the host is being
//! compiled with** (`PROFILE`) and only falls back to the other one as a last
//! resort, with a warning: a release `anvil` that embedded debug guests would
//! start tens of seconds slower (wasmtime compiles the guest unoptimized),
//! which is a failure that is hard to attribute.

use std::env;
use std::path::PathBuf;
use std::process;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    // repo root = crate_dir/../../
    let crate_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let repo_root = crate_dir.ancestors().nth(2).expect("repo root");

    // (output name, target subdir, source dir, suggested command)
    let artifacts: Vec<(&str, &str, &str, &str)> = vec![
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
            "executors/wasm/",
            "cargo build --manifest-path executors/wasm/Cargo.toml",
        ),
    ];

    // The profile cargo is compiling the host with ("debug"/"release");
    // artifacts are taken from it first, the other one is the fallback.
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".into());
    let fallback = if profile == "release" {
        "debug"
    } else {
        "release"
    };
    // The shortest thing that fixes the situation, whichever the profile.
    let recipe = if profile == "release" {
        "make release"
    } else {
        "make build"
    };

    for (name, subdir, dir, command) in &artifacts {
        let dst = out_dir.join(name);
        let path = |p: &str| repo_root.join(dir).join(subdir).join(p).join(name);
        let src = if path(profile).exists() {
            path(profile)
        } else if path(fallback).exists() {
            println!(
                "cargo:warning=the host is being built in '{profile}' but '{name}' only exists \
                 in '{fallback}': that one gets embedded. For a coherent binary: {recipe}"
            );
            path(fallback)
        } else {
            eprintln!(
                "Missing artifact '{name}'. Run first:\n  {command}\n\
                 (or, simpler, `{recipe}` from the repo root)"
            );
            process::exit(1);
        };
        std::fs::copy(&src, &dst).expect("copy artifact into OUT_DIR");
        println!("cargo:rerun-if-changed={}", src.display());
    }
    // Rebuild the host when the artifacts change.
    println!(
        "cargo:rerun-if-changed={}",
        repo_root.join("target/wasm32-wasip2").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        repo_root.join("executors/wasm/target").display()
    );
}