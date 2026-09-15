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
#   2. No function polls against a clock — ZERO, fleet-wide (the ratchet P7-a started reached
#      0 with P7-d; the baseline file is gone).
#   3. No clock-bound wait form decides a request/reply — ZERO: `Wait::Timeout(`,
#      `deadline_after(`, `*_budgeted(`, `recv_until(`, `send_until(`, `recv_matching_until(`
#      are absent from every consumer (a reply is waited for until it arrives or the peer dies —
#      `nexus_ipc::exchange`; pacing is a kernel one-shot timer on a waitset, never a recv
#      timeout). The transport files that IMPLEMENT the wait forms are excluded by path.
# `#[cfg(test)]` modules are skipped. Every run first proves the scanner on fixtures
# (`--self-test` runs only that).
set -euo pipefail
cd "$(dirname "$0")/.."

# Prints "<count> <path>" per file with clock-bound wait forms (code only, tests skipped).
scan_timeouts() {
  python3 - "$@" <<'PY'
import os, re, sys
forms = re.compile(r"Wait::Timeout\(|deadline_after\(|[a-z_]+_budgeted\(|\brecv_until\(|\bsend_until\(|recv_matching_until\(")
EXCLUDE = ("/target/", "/tests/", "/host.rs", "/src/os.rs", "nexus-ipc/src/budget.rs",
           "nexus-ipc/src/os_kernel.rs", "nexus-ipc/src/os_lite.rs", "nexus-ipc/src/reqrep.rs")
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
            n = len(forms.findall(code))
            if n:
                print(f"{n} {p}")
PY
}
ROOTS=(source/services source/drivers source/apps source/init userspace)

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
  printf 'fn t(c: &KernelClient) { let _ = c.recv(Wait::Timeout(Duration::from_millis(5))); }\n' > "$tmp/clean/src/timeout.rs"
  [ "$(scan_timeouts "$tmp/clean" | grep -c 'src/timeout.rs$')" = "1" ] || { echo "[FAIL] wait-not-poll: timeout scanner missed the fixture" >&2; exit 1; }
  echo "[ok]   wait-not-poll: scanner self-test passed (2 poll shapes caught, drain + test module skipped, retired symbol caught, timeout form caught)"
  exit 0
fi

"$0" --self-test >/dev/null

hits=$(retired "${ROOTS[@]}")
if [ -n "$hits" ]; then
  echo "[FAIL] wait-not-poll: a retired spin helper is back (the ONE wait is nexus_ipc::budget):" >&2
  echo "$hits" >&2
  exit 1
fi

hits=$(scan "${ROOTS[@]}")
if [ -n "$hits" ]; then
  echo "[FAIL] wait-not-poll: poll-against-clock function(s) — ZERO remain after TASK-0324 P7-d:" >&2
  echo "$hits" >&2
  echo "       WAIT for the reply (a blocking, EOF-opted receive: the answer or the peer's death); a clock never decides it." >&2
  exit 1
fi
t_hits=$(scan_timeouts "${ROOTS[@]}")
if [ -n "$t_hits" ]; then
  echo "[FAIL] wait-not-poll: clock-bound wait form(s) — ZERO remain after TASK-0324 P7-d (nexus_ipc::exchange waits; pacing is a timer cap on a waitset):" >&2
  echo "$t_hits" >&2
  exit 1
fi
echo "[PASS] wait-not-poll: no retired spin helper, zero poll-against-clock functions, zero clock-bound wait forms"
exit 0
