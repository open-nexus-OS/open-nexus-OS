#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Append one hypothesis-grid NDJSON record to $HYPOTHESIS_LOG
# OWNERS: @tools-team
# STATUS: Functional
# API_STABILITY: Stable
# TEST_COVERAGE: No tests (3-line logging helper)
# ADR: docs/architecture/02-selftest-and-ci.md
#
# Usage: hypothesis-log.sh <hypothesisId> <location> <message> [data-json]
# No-op unless HYPOTHESIS_LOG is set. Record schema matches the writers in
# scripts/qemu-test.sh / scripts/build.sh; decode via docs/testing/run-logs.md.

set -euo pipefail
[[ -z "${HYPOTHESIS_LOG:-}" ]] && exit 0
hid=$1; loc=$2; msg=$3; data=${4:-null}
printf '{"runId":"%s","hypothesisId":"%s","location":"%s","message":"%s","data":%s,"timestamp":%s}\n' \
  "${RUN_ID:-manual}" "$hid" "$loc" "$msg" "$data" "$(date +%s%3N)" >>"$HYPOTHESIS_LOG"
