#!/bin/sh
set -eu

cargo fmt --all
cargo clippy --all-targets --all-features
cargo deny check
