// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO
//! The SDK's copy of the WIT is the bridge's copy.
//!
//! `wit/anvil-step.wit` is here because `wit_bindgen::generate!` resolves its
//! path against this crate, and the source of truth is the bridge's
//! (`executors/wasm/wit/`, see `executors/wasm/src/main.rs`). Two copies of the
//! contract that can drift are exactly what ADR-0020 §4e warns about, so this
//! is the net: the copies move in the same commit or this test goes red.
//!
//! Outside the repo —in the published crate— the bridge's file is not there and
//! this test does not build. That is fine: it guards the repo, which is where
//! the two files can be edited apart.

#[test]
fn the_sdk_wit_is_the_bridge_wit() {
    let ours = include_str!("../wit/anvil-step.wit");
    let Ok(bridge) = std::fs::read_to_string("../../wasm/wit/anvil-step.wit") else {
        // Outside the repo there is nothing to compare against, and the crate
        // still has to be testable by whoever downloaded it.
        return;
    };
    // `assert!` and not `assert_eq!`: dumping two copies of the contract into
    // the failure makes the one line that changed impossible to find.
    assert!(
        ours == bridge,
        "the SDK's copy of the WIT and the bridge's have drifted: copy \
         executors/wasm/wit/anvil-step.wit over executors/rust/anvil-step/wit/"
    );
}
