#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: the IPC payload bounds have ONE value on both sides of the ABI
# (TASK-0054C P3b, RFC-0096).
#
# The kernel is the DEFINING side of the ABI and deliberately does not depend on
# `nexus-abi` — which is why syscall numbers live as mirrored literals today and
# nothing notices when one side drifts. `IPC_SHORT_MAX` and `IPC_PAYLOAD_MAX` are
# worse than a syscall number if they drift: a userspace receive buffer sized to
# a stale cap turns an oversize frame into a corrupt one, which is exactly how an
# oversize OTA frame once came back as "bad signature" (RFC-0096 §Copies).
#
# So the mirror is mechanical, not remembered. This gate compares the two
# definitions and fails on any difference.
#
# Scope + limits: it compares the DECLARED literals, not what the compiler
# eventually folds. A third copy introduced somewhere else is caught by rule 2,
# which forbids the bare `8 * 1024` / `8192` frame cap outside the two owners.
set -euo pipefail

cd "$(dirname "$0")/.."

KERNEL_SRC="source/kernel/neuron/src/ipc/payload.rs"
ABI_SRC="source/libs/nexus-abi/src/lib.rs"
fail=0

value_of() { # <file> <const-name> -> the literal, evaluated (`pub` or `pub(crate)`)
  local v
  v=$(grep -oE "pub(\(crate\))? const $2: usize = [^;]+;" "$1" | head -1 | sed -E "s/.*= (.*);/\1/")
  [ -n "$v" ] && echo $(( v )) || echo ""
}

for f in "$KERNEL_SRC" "$ABI_SRC"; do
  [ -f "$f" ] || { echo "[FAIL] ipc-bounds: $f not found" >&2; exit 1; }
done

# --- self-test: the gate must actually catch a drift ---------------------------
if [ "${1:-}" == "--self-test" ]; then
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  printf 'pub const IPC_SHORT_MAX: usize = 32;\npub const IPC_PAYLOAD_MAX: usize = 8 * 1024;\n' > "$tmp/k.rs"
  printf 'pub const IPC_SHORT_MAX: usize = 64;\npub const IPC_PAYLOAD_MAX: usize = 8 * 1024;\n' > "$tmp/a.rs"
  got_k=$(value_of "$tmp/k.rs" IPC_SHORT_MAX)
  got_a=$(value_of "$tmp/a.rs" IPC_SHORT_MAX)
  if [ "$got_k" == "32" ] && [ "$got_a" == "64" ] && [ "$got_k" != "$got_a" ]; then
    echo "[PASS] ipc-bounds self-test: a drifted mirror is detected (32 vs 64)"
  else
    echo "[FAIL] ipc-bounds self-test: the scanner did not read the fixture (k='$got_k' a='$got_a')" >&2
    exit 1
  fi
  exit 0
fi

# --- rule 1: the two sides agree -----------------------------------------------
for name in IPC_SHORT_MAX IPC_PAYLOAD_MAX; do
  k=$(value_of "$KERNEL_SRC" "$name")
  a=$(value_of "$ABI_SRC" "$name")
  if [ -z "$k" ] || [ -z "$a" ]; then
    echo "[FAIL] ipc-bounds: $name missing (kernel='${k:-none}' abi='${a:-none}')" >&2
    echo "       both sides must declare it: $KERNEL_SRC and $ABI_SRC" >&2
    fail=1
  elif [ "$k" != "$a" ]; then
    echo "[FAIL] ipc-bounds: $name drifted — kernel=$k abi=$a" >&2
    echo "       the kernel enforces it, userspace sizes buffers by it; they cannot differ." >&2
    fail=1
  fi
done

# --- rule 1b: the waitset member bound (TASK-0067) -----------------------------
# init's responder holds one member per control channel plus its respawn timer; the kernel
# refuses a member past the bound. A userspace copy that drifts below the kernel's lets
# init's host test pass while the boot silently loses its timer (measured on smp1: a crashed
# service was never restarted).
k=$(value_of "source/kernel/neuron/src/waitset.rs" MAX_WAITSET_MEMBERS)
a=$(value_of "$ABI_SRC" WAITSET_MEMBERS_MAX)
if [ -z "$k" ] || [ -z "$a" ]; then
  echo "[FAIL] ipc-bounds: waitset member bound missing (kernel='${k:-none}' abi='${a:-none}')" >&2
  fail=1
elif [ "$k" != "$a" ]; then
  echo "[FAIL] ipc-bounds: waitset member bound drifted — kernel=$k abi=$a" >&2
  fail=1
fi

# --- rule 2: nobody re-declares the frame cap ----------------------------------
# The private `8 * 1024` literals P3b/P2-g deleted (ipc_msg.rs, ipc_recv_v2.rs,
# ipc/endpoint.rs, selftest/mod.rs, statefsd, keystored, vfsd) must not grow back
# under a new name, anywhere a frame is sized.
# Frame-CAP names only. A scratch-buffer size is not a transport cap: app-host's
# `REPLY_BUF` sizes a stack array at eight call sites, so folding it into the ABI
# constant would put 8 KiB on the stack per effect call (measured, not assumed).
hits=$(grep -rnE '(MAX_FRAME_BYTES|MAX_FRAME|MAX_REQUEST_FRAME|MAX_PAYLOAD_BYTES|IPC_MAX_FRAME[A-Z_]*)[^A-Z_]*(=|:)[^=]*(8 \* 1024|8192|512)' \
           source/kernel/neuron/src source/services source/drivers userspace \
           2>/dev/null | grep -v '^\s*//' || true)
if [ -n "$hits" ]; then
  echo "[FAIL] ipc-bounds: a private frame cap is back — use nexus_abi::IPC_PAYLOAD_MAX:" >&2
  echo "$hits" >&2
  fail=1
fi

# --- rule 3: an os-lite server loop allocates nothing per request ---------------
# The bump allocator never frees, so a `Vec` per request is a countdown, not a
# cost: inputd took ~400 HID batches a second under a pointer drag and walked its
# 384 KiB heap to `alloc_error` in under twenty seconds — and a dead inputd takes
# the whole input chain with it (hidrawd `tx hz=0`, no click reaches the greeter).
# The receive inside a service loop must be the `_into` form with a buffer hoisted
# out of the loop. See TASK-0054C P2-g.
# Matched by SHAPE, not by a list of names: the first cut of this rule spelled out
# the receives it knew about and missed `recv_with_header_meta`, leaving samgrd and
# execd — two core services — leaking a `Vec` per request (TASK-0054C P5b). Any
# `.recv*(` that is not an `_into` form returns a buffer, so it is caught here
# whatever it is called.
hits=$(grep -rnE '(server|client)\.recv[a-z_]*\(' \
           source/services source/drivers \
           --include='os_lite.rs' --include='os_stub.rs' --include='local_ipc.rs' \
           2>/dev/null | grep -v '_into' | grep -v '^\s*//' || true)
if [ -n "$hits" ]; then
  echo "[FAIL] ipc-bounds: an os-lite loop uses the ALLOCATING receive (bump heap never frees):" >&2
  echo "$hits" >&2
  echo "       hoist one buffer out of the loop and use the _into form." >&2
  fail=1
fi

if [ "$fail" == "0" ]; then
  echo "[PASS] ipc-bounds: IPC_SHORT_MAX + IPC_PAYLOAD_MAX + the waitset member bound identical in kernel and ABI, no private frame cap"
fi
exit "$fail"
