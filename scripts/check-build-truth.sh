#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Structure gate (TASK-0324 P0): per-service cargo features and ELF
# paths have ONE home — `[package.metadata.nexus-service]` resolved by
# scripts/discover-services.sh (build_service in scripts/build.sh). A literal
# `--features os-lite`, a per-service `*_CARGO_FLAGS` override, or a
# `target/<triple>/release/<svc>` ELF path in any build script is the dual
# structure that shipped a 2D gpud on a GL device (2026-09-09). Runs in
# `just check`.
set -euo pipefail
cd "$(dirname "$0")/.."
fail=0
# Scripts that may build services: only through build_service().
for f in scripts/build.sh scripts/qemu-launcher.sh scripts/qemu-test.sh Makefile; do
  [[ -f "$f" ]] || continue
  if grep -nE -- '--features (os-lite|"os-lite")' "$f" | grep -v 'discover-services\|build_service' >/dev/null; then
    echo "[FAIL] build-truth: $f hard-codes a service feature set (use build_service / discover-services.sh):" >&2
    grep -nE -- '--features (os-lite|"os-lite")' "$f" >&2
    fail=1
  fi
  if grep -n '_CARGO_FLAGS' "$f" >/dev/null; then
    echo "[FAIL] build-truth: $f uses a per-service *_CARGO_FLAGS override (features live in the crate manifest):" >&2
    grep -n '_CARGO_FLAGS' "$f" >&2
    fail=1
  fi
done
# The keyed artifact is the only ELF path for services/payloads.
if grep -nE 'release/\$svc"?$|release/\$svc[^_a-zA-Z]' scripts/build.sh | grep -v 'cargo_out=' >/dev/null; then
  echo "[FAIL] build-truth: scripts/build.sh addresses a service ELF by target/…/release/<svc> (use service_elf_path):" >&2
  grep -nE 'release/\$svc' scripts/build.sh >&2
  fail=1
fi
if [[ "$fail" -ne 0 ]]; then exit 1; fi
echo "[PASS] build-truth: service features + ELF paths resolve through the manifest SSOT only"
