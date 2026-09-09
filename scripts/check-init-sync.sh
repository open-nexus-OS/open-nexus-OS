#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Structure gate (TASK-0324 P2, RFC-0093 §2, ADR-0062): a service's
# readiness has ONE home — `nexus_service_entry::ready(marker)`, which prints the
# `<svc>: ready` marker AND announces `@ready` to init — and `init: up <svc>` is
# emitted ONLY by init's responder on that announce. A `": ready"` marker printed
# through any other call, or an `init: up` written anywhere else, is the dual
# structure that made `init: up` a lie (printed at resume, before the service
# ever ran). Runs in `just check`.
set -euo pipefail
cd "$(dirname "$0")/.."
fail=0

# 1. `<svc>: ready` markers only through nexus_service_entry::ready(...).
#    Matches the OS print funnels (debug_println / emit_line / emit / trace_line /
#    debug_print) carrying a `: ready"` or `: ready\n"` literal.
if grep -rnE '(debug_println|emit_line|emit|trace_line|debug_print)\(\s*"[a-z0-9_-]+: ready(\\n)?"' \
     source/services source/drivers source/apps 2>/dev/null \
   | grep -v 'nexus-service-entry\|std_server.rs' >/dev/null; then
  echo "[FAIL] init-sync: a ready marker bypasses nexus_service_entry::ready():" >&2
  grep -rnE '(debug_println|emit_line|emit|trace_line|debug_print)\(\s*"[a-z0-9_-]+: ready(\\n)?"' \
     source/services source/drivers source/apps 2>/dev/null | grep -v 'nexus-service-entry\|std_server.rs' >&2
  fail=1
fi

# 2. `init: up ` is emitted only by the responder's `@ready` arm.
#    std_server.rs is the host-only std simulation of init (no control channel).
if grep -rn 'init: up ' source/init/nexus-init/src 2>/dev/null \
   | grep -v 'bootstrap/responder.rs\|std_server.rs\|^[^:]*:[0-9]*:\s*//' >/dev/null; then
  echo "[FAIL] init-sync: 'init: up ' emitted outside the responder @ready arm:" >&2
  grep -rn 'init: up ' source/init/nexus-init/src 2>/dev/null \
   | grep -v 'bootstrap/responder.rs\|std_server.rs\|^[^:]*:[0-9]*:\s*//' >&2
  fail=1
fi

if [[ "$fail" == "0" ]]; then
  echo "[PASS] init-sync: ready markers funnel through nexus_service_entry::ready(); init: up only from @ready"
fi
exit "$fail"
