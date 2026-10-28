#!/bin/sh
cd "$(dirname "$0")" || exit 1
if ! command -v cargo >/dev/null 2>&1; then echo 'Install Rust from https://rustup.rs/ and restart Terminal.'; exit 1; fi
if [ -z "$TELOXIDE_TOKEN" ]; then echo 'Set TELOXIDE_TOKEN in this terminal first.'; exit 1; fi
cargo run --release
