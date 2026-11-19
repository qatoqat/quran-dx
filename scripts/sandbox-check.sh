#!/usr/bin/env bash
# Sandbox-only helper: the cloud sandbox can't download Rust 1.100 or the wasm target,
# so it checks with the installed stable toolchain on the native host target.
# On a real machine just use `cargo check` / `dx serve` (rust-toolchain.toml pins 1.100).
set -euo pipefail
cd "$(dirname "$0")/.."
export RUSTUP_TOOLCHAIN=stable
export CARGO_BUILD_JOBS=2
cmd="${1:-check}"; shift || true
# Default to the whole workspace unless a package is selected.
scope=(--workspace)
for a in "$@"; do [[ "$a" == "-p" || "$a" == --package* ]] && scope=(); done
case "$cmd" in
  check)  cargo check --ignore-rust-version "${scope[@]}" "$@" ;;
  test)   cargo test  --ignore-rust-version "${scope[@]}" "$@" ;;
  clippy) cargo clippy --ignore-rust-version "${scope[@]}" --all-targets "$@" -- -D warnings ;;
  *)      cargo "$cmd" --ignore-rust-version "$@" ;;
esac
