#!/bin/sh
set -eu

PROJECT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$PROJECT_DIR"

cargo build --release --bin nanokvm-rust
cp target/release/nanokvm-rust "$PROJECT_DIR/nanokvm-rust"
printf 'Built %s/nanokvm-rust\n' "$PROJECT_DIR"
