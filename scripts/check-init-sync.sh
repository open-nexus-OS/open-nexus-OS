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

# 3. Routing v1 is gone (RFC-0093 §1, TASK-0324 P3): the nonce-less ask and the
#    32-frame "drain stale replies" prologue that compensated for it.
if grep -rn 'query_route' source userspace 2>/dev/null | grep -v '^[^:]*:[0-9]*:\s*//' >/dev/null; then
  echo "[FAIL] routing-v2: 'query_route' (nonce-less routing v1) is back:" >&2
  grep -rn 'query_route' source userspace 2>/dev/null | grep -v '^[^:]*:[0-9]*:\s*//' >&2
  fail=1
fi

# 4. The guards that existed ONLY because a route answer could not be correlated.
#    windowd rejecting an answer equal to its own inbox is the confused-waiter workaround;
#    with a mandatory nonce the library refuses such an answer instead.
if grep -rn 'SERVER_RECV_SLOT\|ALIAS_REPORTED\|note_server_recv_slot' source/services/windowd/src 2>/dev/null \
   | grep -v '^[^:]*:[0-9]*:\s*//\|///' >/dev/null; then
  echo "[FAIL] routing-v2: windowd route-alias guards are back (nonce makes them impossible):" >&2
  grep -rn 'SERVER_RECV_SLOT\|ALIAS_REPORTED\|note_server_recv_slot' source/services/windowd/src >&2
  fail=1
fi

# 5. init answers a route ask ONLY through the nonce-echoing helper; a raw reply send in
#    the responder is the path that used to drop replies silently into a full queue.
if grep -n 'encode_route_rsp' source/init/nexus-init/src/bootstrap/responder.rs 2>/dev/null >/dev/null; then
  echo "[FAIL] routing-v2: responder builds a route reply outside route_reply::send_route_rsp:" >&2
  grep -n 'encode_route_rsp' source/init/nexus-init/src/bootstrap/responder.rs >&2
  fail=1
fi

if [[ "$fail" == "0" ]]; then
  echo "[PASS] init-sync: ready markers funnel through nexus_service_entry::ready(); init: up only from @ready"
  echo "[PASS] routing-v2: no nonce-less routing, no alias guards, one route-reply path"
fi
exit "$fail"
