#!/usr/bin/env sh
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

set -eu

repo_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$repo_root"

cargo test -p input_v1_0_host -- --nocapture
cargo test -p hidrawd -- --nocapture
cargo test -p touchd -- --nocapture
cargo test -p inputd -- --nocapture
