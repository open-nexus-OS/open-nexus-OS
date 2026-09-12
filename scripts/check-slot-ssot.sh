#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Slot-SSOT gate (TASK-0324 P4, RFC-0093 §4). Capability slot numbers have ONE home:
# `nexus-service-topology` for every slot init provisions into a service, and `nexus-abi` for the
# slots the kernel installs itself (a task's bootstrap endpoint, init's endpoint factory — mirrored
# there like the syscall numbers). P4a-P4f moved the fleet onto that home and ratcheted the
# positional declarations 191 -> 0; since P4f-6 the ratchet is an absolute rule. No Rust source
# outside those two crates may carry a capability slot as a number:
#   const <NAME>SLOT / <NAME>_EP: u32|Handle|Cap = <literal>
#   let <..slot..> = <literal>
#   new_with_slots(<non-zero literal>, ...)        (0 names the kernel bootstrap slot / "none")
#   cap_transfer_to_slot(..., <literal>)
# Every run first proves the scanner on fixtures (`--self-test` runs only that).
set -euo pipefail
cd "$(dirname "$0")/.."

SCAN=(source/services source/drivers source/apps source/init source/libs userspace)

scan() {
  python3 - "$@" <<'PY'
import os, re, sys
EXCLUDE = ("source/libs/nexus-service-topology/", "source/libs/nexus-abi/", "/target/", "/tests/")
NUM = r"(?:0x[0-9a-fA-F_]+|\d[\d_]*)"
NONZERO = r"(?:0x[0-9a-fA-F_]*[1-9a-fA-F][0-9a-fA-F_]*|[1-9][\d_]*)"
RULES = [
    ("positional slot const",
     re.compile(r"\bconst\s+[A-Z0-9_]*(?:SLOT|_EP)\s*:\s*(?:u32|Handle|Cap)\s*=\s*" + NUM + r"\s*;")),
    ("positional slot let",
     re.compile(r"\blet\s+(?:mut\s+)?\w*slot\w*\s*(?::\s*u32)?\s*=\s*" + NUM + r"\s*;")),
    ("literal slot in new_with_slots",
     re.compile(r"new_with_slots\(\s*" + NONZERO + r"\s*,|new_with_slots\([^()]*,\s*" + NONZERO + r"\s*\)")),
]
PIN = re.compile(r"cap_transfer_to_slot\(([^;{}]*?)\)", re.S)
hits = []
for root in sys.argv[1:]:
    for dirpath, _, files in os.walk(root):
        rel = dirpath.replace(os.sep, "/") + "/"
        if any(part in rel for part in EXCLUDE):
            continue
        for name in sorted(files):
            if not name.endswith(".rs"):
                continue
            path = os.path.join(dirpath, name)
            lines = open(path, encoding="utf-8", errors="ignore").read().splitlines()
            code = [line.split("//", 1)[0] for line in lines]
            for lineno, text in enumerate(code, 1):
                for what, rule in RULES:
                    if rule.search(text):
                        hits.append(f"{path}:{lineno}: {what}: {lines[lineno - 1].strip()}")
            joined = "\n".join(code)
            for m in PIN.finditer(joined):
                args = [a.strip() for a in m.group(1).split(",") if a.strip()]
                if args and re.fullmatch(NUM, args[-1]):
                    lineno = joined[: m.start()].count("\n") + 1
                    hits.append(f"{path}:{lineno}: literal slot in cap_transfer_to_slot: {args[-1]}")
for hit in hits:
    print(hit)
sys.exit(1 if hits else 0)
PY
}

self_test() {
  local tmp
  tmp=$(mktemp -d)
  mkdir -p "$tmp/case"
  cat > "$tmp/case/violations.rs" <<'RS'
const REPLY_RECV_SLOT: u32 = 0x17;
const BOOTSTRAP_EP: u32 = 0;
const IRQ_NOTIFY_SLOT: Cap = 2;
fn f() {
    let reply_send_slot: u32 = 0x6;
    let _ = KernelClient::new_with_slots(0x9, 0xA);
    let _ = nexus_abi::cap_transfer_to_slot(
        pid,
        cap,
        Rights::SEND,
        0x31,
    );
}
RS
  cat > "$tmp/case/declared.rs" <<'RS'
const REPLY_RECV_SLOT: u32 = nexus_service_topology::slots::selftest_client::REPLY.recv;
const BOOTSTRAP_EP: u32 = nexus_abi::BOOTSTRAP_CAP_SLOT;
// const OLD_SLOT: u32 = 5; (a comment is not code)
fn g() {
    let reply = nexus_service_topology::slots::dsoftbusd::REPLY;
    let _ = KernelClient::new_with_slots(route.send, route.recv);
    let _ = nexus_abi::cap_transfer_to_slot(pid, cap, Rights::SEND, slots.send);
    let _ = KernelClient::new_with_slots(send, 0);
}
RS
  local out rc=0
  out=$(scan "$tmp/case") || rc=$?
  rm -rf "$tmp"
  local count
  count=$(printf '%s\n' "$out" | grep -c . || true)
  if [[ "$rc" != "1" || "$count" != "6" ]] || printf '%s\n' "$out" | grep -q "declared.rs"; then
    echo "[FAIL] slot-ssot self-test: expected 6 reports, all from violations.rs (rc=$rc):" >&2
    printf '%s\n' "$out" >&2
    return 1
  fi
  echo "[PASS] slot-ssot self-test: every positional shape reported, declared forms ignored"
}

case "${1:-}" in
  --self-test) self_test; exit $? ;;
  "") ;;
  *) echo "usage: $0 [--self-test]" >&2; exit 2 ;;
esac

self_test >/dev/null || { self_test; exit 1; }
rc=0
out=$(scan "${SCAN[@]}") || rc=$?
if [[ "$rc" == "0" ]]; then
  echo "[PASS] slot-ssot: no capability slot number outside nexus-service-topology / nexus-abi (scanner self-test ok)"
  exit 0
fi
echo "[FAIL] slot-ssot: capability slot numbers outside their home — declare them in nexus-service-topology (or nexus-abi for kernel-installed slots):" >&2
printf '%s\n' "$out" >&2
exit 1
