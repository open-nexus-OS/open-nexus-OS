#!/bin/sh
set -euo pipefail

cargo fmt --all
# Host-first lint; exclude no_std kernel (needs -Z build-std)
cargo clippy \
  --workspace \
  --all-targets \
  --all-features \
  --exclude neuron \
  -- -D warnings

# Optional: lint kernel separately (only if explicitly desired)
#   KERNEL_LINT=1 NIGHTLY=nightly-2025-01-15 ./scripts/fmt-clippy-deny.sh
if [ "${KERNEL_LINT:-0}" = "1" ]; then
  NIGHTLY="${NIGHTLY:-nightly-2025-01-15}"
  cargo +"${NIGHTLY}" clippy \
    -Z build-std=core,alloc -Z build-std-features=panic_immediate_abort \
    --target riscv64imac-unknown-none-elf -p neuron -- -D warnings || true
fi

command -v cargo-deny >/dev/null 2>&1 && cargo deny check --config config/deny.toml || echo "warn: cargo-deny not found; skipping"
