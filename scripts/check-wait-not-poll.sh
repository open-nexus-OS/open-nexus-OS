#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Wait-not-poll gate (TASK-0324 P7, RFC-0093 §1 "parked, never polled"). A reply is
# WAITED for — the kernel wakes the waiter when the frame arrives — never polled. The gate
# flags every function body that combines a non-blocking IPC attempt (`Wait::NonBlocking`,
# `IPC_SYS_NONBLOCK`, `*_nb(`) with `yield_()` AND a wall-clock or attempt bound (`nsec()`,
# `deadline`, `for _ in 0..N`): that is a poll against a clock, the shape that made a live peer
# under host load indistinguishable from a dead one. Two rules:
#   1. The retired spin helpers stay retired (`retry_ipc_until`, `Clock::yield_now`,
#      `recv_match_bounded`, `routing_v1_get`, `wait_for_slots_ready`).
#   2. Per-file poll counts may only SHRINK against config/wait-not-poll-baseline.txt (the
#      ratchet P7-a starts and P7-b/P7-c drive to 0); a file not in the baseline must be at 0.
# `#[cfg(test)]` modules are skipped. Every run first proves the scanner on fixtures
# (`--self-test` runs only that).
set -euo pipefail
cd "$(dirname "$0")/.."

BASELINE=config/wait-not-poll-baseline.txt
ROOTS=(source/services source/drivers source/apps userspace)

# Prints "<count> <path>: <fn>[,<fn>...]" per file with hits (sorted), exit 0 always.
scan() {
  python3 - "$@" <<'PY'
import os, re, sys
EXCLUDE = ("/target/", "/tests/", "/host.rs", "/src/os.rs")
NB_RECV = re.compile(r"recv\w*\(\s*(?:Ipc)?Wait::NonBlocking|ipc_recv_v[12]_nb\(|ipc_recv_v[12]\([^;]*?IPC_SYS_NONBLOCK", re.S)
NB_SEND = re.compile(r"send\w*\([^;]*?(?:Ipc)?Wait::NonBlocking\s*\)|ipc_send_v1_nb\(|ipc_send_v1\([^;]*?IPC_SYS_NONBLOCK", re.S)
YIELD = re.compile(r"\byield_\(\)")
BOUND = re.compile(r"\bnsec\(\)|deadline|\bfor\s+_\s+in\s+0\.\.")
FN = re.compile(r"\bfn\s+(\w+)\s*[<(]")
def strip_tests(src):
    out = []; i = 0
    while True:
        m = re.search(r"#\[cfg\([^)]*test[^)]*\)\]\s*mod\s+\w+\s*\{", src[i:])
        if not m:
            out.append(src[i:]); break
        out.append(src[i:i + m.start()]); j = i + m.end(); d = 1
        while j < len(src) and d:
            d += {"{": 1, "}": -1}.get(src[j], 0); j += 1
        i = j
    return "".join(out)
def bodies(src):
    for m in FN.finditer(src):
        b = src.find("{", m.end()); semi = src.find(";", m.end())
        if b < 0 or (0 <= semi < b):
            continue
        d = 0; j = b
        while j < len(src):
            d += 1 if src[j] == "{" else -1 if src[j] == "}" else 0
            j += 1
            if d == 0:
                break
        yield m.group(1), src[b:j]
per_file = {}
for root in sys.argv[1:]:
    for dp, _, fs in os.walk(root):
        for f in sorted(fs):
            if not f.endswith(".rs"):
                continue
            p = os.path.join(dp, f).replace(os.sep, "/")
            if any(x in p for x in EXCLUDE):
                continue
            src = strip_tests(open(p, encoding="utf-8", errors="ignore").read())
            code = "\n".join(l.split("//", 1)[0] for l in src.splitlines())
            fns = [n for n, body in bodies(code)
                   if YIELD.search(body) and BOUND.search(body) and (NB_RECV.search(body) or NB_SEND.search(body))]
            if fns:
                per_file[p] = fns
for p in sorted(per_file):
    print(f"{len(per_file[p])} {p}: {','.join(per_file[p])}")
PY
}

retired() {
  # Rule 1: the spin helpers do not come back (declarations and calls; a comment naming
  # them as history is not a resurrection).
  grep -rnE "\b(retry_ipc_until|retry_ipc_budgeted|recv_match_bounded|routing_v1_get|wait_for_slots_ready)\b|fn yield_now\(" \
    --include='*.rs' "$@" 2>/dev/null | grep -v '/tests/' | grep -vE '^[^:]+:[0-9]+:[[:space:]]*//' || true
}

if [ "${1:-}" = "--self-test" ]; then
  tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
  mkdir -p "$tmp/svc/src" "$tmp/clean/src"
  cat > "$tmp/svc/src/os_lite.rs" <<'RS'
fn poll_reply(client: &KernelClient) -> Option<Vec<u8>> {
    let deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(500_000_000);
    loop {
        match client.recv(Wait::NonBlocking) {
            Ok(v) => return Some(v),
            Err(_) => { if nexus_abi::nsec().unwrap_or(0) >= deadline { return None; } let _ = yield_(); }
        }
    }
}
fn raw_poll(slot: u32) -> bool {
    for _ in 0..128 {
        if nexus_abi::ipc_recv_v1(slot, &mut h, &mut b, nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE, 0).is_ok() { return true; }
        let _ = yield_();
    }
    false
}
fn event_loop_drain(client: &KernelClient) {
    // a drain without a clock or attempt bound is not a poll against a clock
    while let Ok(f) = client.recv(Wait::NonBlocking) { handle(f); }
}
#[cfg(test)]
mod tests {
    fn in_test_only() { let deadline = nsec(); let _ = c.recv(Wait::NonBlocking); let _ = yield_(); }
}
RS
  cat > "$tmp/clean/src/os_lite.rs" <<'RS'
fn wait_reply(client: &KernelClient, deadline: u64) -> Option<Vec<u8>> {
    let clock = nexus_ipc::budget::OsClock;
    nexus_ipc::budget::recv_until(&clock, client, deadline).ok()
}
RS
  got=$(scan "$tmp/svc")
  [ "$got" = "2 $tmp/svc/src/os_lite.rs: poll_reply,raw_poll" ] || {
    echo "[FAIL] wait-not-poll: scanner self-test expected 2 hits (poll_reply,raw_poll), got: $got" >&2; exit 1; }
  [ -z "$(scan "$tmp/clean")" ] || { echo "[FAIL] wait-not-poll: scanner flags a clean wait" >&2; exit 1; }
  printf '// history: retry_ipc_until once lived here\nfn x() { retry_ipc_until(&c, d, || op()); }\n' > "$tmp/clean/src/retired.rs"
  [ "$(retired "$tmp/clean" | wc -l)" = "1" ] || { echo "[FAIL] wait-not-poll: retired-symbol rule must catch the call and skip the comment" >&2; exit 1; }
  echo "[ok]   wait-not-poll: scanner self-test passed (2 poll shapes caught, drain + test module skipped, retired symbol caught)"
  exit 0
fi

"$0" --self-test >/dev/null

hits=$(retired "${ROOTS[@]}")
if [ -n "$hits" ]; then
  echo "[FAIL] wait-not-poll: a retired spin helper is back (the ONE wait is nexus_ipc::budget):" >&2
  echo "$hits" >&2
  exit 1
fi

[ -f "$BASELINE" ] || { echo "[FAIL] wait-not-poll: missing $BASELINE" >&2; exit 1; }
status=0
total=0
while IFS= read -r line; do
  [ -z "$line" ] && continue
  count=${line%% *}; rest=${line#* }; path=${rest%%:*}; fns=${rest#*: }
  total=$((total + count))
  allowed=$(awk -v p="$path" '$2 == p { print $1 }' "$BASELINE")
  allowed=${allowed:-0}
  if [ "$count" -gt "$allowed" ]; then
    echo "[FAIL] wait-not-poll: $path has $count poll-against-clock function(s) (baseline $allowed): $fns" >&2
    echo "       WAIT for the reply (nexus_ipc::budget::recv_until / raw::recv_budgeted / Wait::Timeout); the deadline is a liveness bound, never a spin budget." >&2
    status=1
  fi
done < <(scan "${ROOTS[@]}")
# The ratchet only shrinks: a baseline entry above the real count is stale and must be lowered.
while read -r allowed path; do
  [ -z "$path" ] && continue
  case "$allowed" in \#*) continue ;; esac
  actual=$(scan "${ROOTS[@]}" | awk -v p="$path" '{ split($2, a, ":"); if (a[1] == p) print $1 }')
  actual=${actual:-0}
  if [ "$actual" -lt "$allowed" ]; then
    echo "[FAIL] wait-not-poll: $path is at $actual but the baseline still allows $allowed — lower the baseline (ratchet)" >&2
    status=1
  fi
done < "$BASELINE"
baseline_total=$(awk '{ s += $1 } END { print s + 0 }' "$BASELINE")
[ "$status" -eq 0 ] && echo "[PASS] wait-not-poll: no retired spin helper; poll-against-clock ratchet $total/$baseline_total (only shrinks)"
exit $status
