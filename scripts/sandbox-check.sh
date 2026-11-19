#!/usr/bin/env bash
# Sandbox-only helper: the cloud sandbox can't download Rust 1.100 or the wasm target,
# so it checks with the installed stable toolchain on the native host target.
# On a real machine just use `cargo check` / `dx serve` (rust-toolchain.toml pins 1.100).
set -euo pipefail
cd "$(dirname "$0")/.."
export RUSTUP_TOOLCHAIN=stable
export CARGO_BUILD_JOBS=2
cmd="${1:-check}"; shift || true
case "$cmd" in
  check)  cargo check --ignore-rust-version --workspace "$@" ;;
  test)   cargo test  --ignore-rust-version --workspace "$@" ;;
  clippy) cargo clippy --ignore-rust-version --workspace --all-targets "$@" -- -D warnings ;;
  *)      cargo "$cmd" --ignore-rust-version "$@" ;;
esac
