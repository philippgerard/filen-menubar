#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
manifest="${repo_root}/third-party/rust/Cargo.toml"
export CARGO_TARGET_DIR="${repo_root}/src-tauri/target"
cargo fmt --manifest-path "$manifest" --all -- --check
cargo test --manifest-path "$manifest" --locked -p glib --release --lib variant_iter::tests -- --test-threads=1
cargo test --manifest-path "$manifest" --locked -p glib-macros --tests -- --test-threads=1
cargo test --manifest-path "$manifest" --locked -p gtk3-macros --test diagnostics -- --test-threads=1
