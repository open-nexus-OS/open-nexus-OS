#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: there is exactly ONE payload-VMO header and ONE status table
# (TASK-0033 P1, RFC-0097).
#
# The same 16 bytes served two protocols under two magics for a year: `NXVR` for
# the VFS splice read and `NXPL` for bundlemgrd's payload ops. That was not
# merely untidy. `NXPL` carried a private `u8` status space whose values
# collided with bundlemgrd's generic reply statuses IN THE SAME MODULE —
# `STATUS_MALFORMED` and `PAYLOAD_STATUS_OK` were both `1` — so a malformed
# request wrote a header that read as SUCCESS, and execd's own status check
# could never fire for it.
#
# A second magic is how that comes back. So the magic is counted, not
# remembered: the header codec must exist in exactly one module, and the status
# table it carries in exactly one enum.
#
# Scope + limits: this is a declaration gate, not a semantic one. It proves
# nobody re-declared the codec or the table; it cannot prove a caller passes the
# right code. That is what the unit tests in `payload_vmo.rs` and the boot
# ladder are for.
set -euo pipefail

cd "$(dirname "$0")/.."

CODEC="source/libs/nexus-wire/src/payload_vmo.rs"
TABLE="source/libs/nexus-wire/src/status.rs"
fail=0

# Source files only — never this script, and never the docs that EXPLAIN the
# retired magic (the RFC and the ledger name `NXPL` on purpose). Scanned from
# the WORKING TREE, not the index: a new file is exactly the case this gate
# exists for, and an untracked one must not slip past it.
sources() {
  find source userspace tests tools -name '*.rs' -type f -not -path '*/target/*' | sort
}

# --- self-test: the gate must actually catch a second declaration -------------
if [ "${1:-}" == "--self-test" ]; then
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  printf 'pub const MAGIC: [u8; 4] = *b"NXVR";\n' > "$tmp/one.rs"
  printf 'pub const OTHER: [u8; 4] = *b"NXVR";\n' > "$tmp/two.rs"
  n=$(grep -lE '\*b"NXVR"' "$tmp"/*.rs | wc -l)
  if [ "$n" == "2" ]; then
    echo "[PASS] payload-vmo self-test: a second magic declaration is detected"
  else
    echo "[FAIL] payload-vmo self-test: the scanner missed the duplicate (n=$n)" >&2
    exit 1
  fi
  exit 0
fi

for f in "$CODEC" "$TABLE"; do
  [ -f "$f" ] || { echo "[FAIL] payload-vmo: $f not found" >&2; exit 1; }
done

# --- rule 1: the header magic is declared exactly once, in the codec ----------
magic_files=$(sources | xargs grep -lE '\*b"NXVR"' || true)
magic_count=$(echo "$magic_files" | grep -c . || true)
if [ "$magic_count" != "1" ] || [ "$magic_files" != "$CODEC" ]; then
  echo "[FAIL] payload-vmo: the NXVR magic must be declared only in $CODEC, found:" >&2
  echo "$magic_files" >&2
  fail=1
fi

# --- rule 2: the retired magic is gone from every code path -------------------
# It may still appear in the codec's own negative test (a stale NXPL header must
# not decode) and in prose; anywhere else is a resurrected second codec.
stale=$(sources | grep -v "^$CODEC\$" | xargs grep -lE '\*b"NXPL"|b"NXPL"' || true)
if [ -n "$stale" ]; then
  echo "[FAIL] payload-vmo: the retired NXPL magic is back in:" >&2
  echo "$stale" >&2
  fail=1
fi

# --- rule 3: the retired private status space stays retired -------------------
retired=$(sources | xargs grep -lE 'PAYLOAD_STATUS_(OK|UNKNOWN|TOO_LARGE|DIGEST|NOT_ARMED)\b' \
  | grep -v "^$CODEC\$" | grep -v "^$TABLE\$" || true)
if [ -n "$retired" ]; then
  echo "[FAIL] payload-vmo: the retired PAYLOAD_STATUS_* space is referenced in:" >&2
  echo "$retired" >&2
  fail=1
fi

# --- rule 4: the status table is declared exactly once ------------------------
table_files=$(sources | xargs grep -lE '^pub enum VfsError' || true)
table_count=$(echo "$table_files" | grep -c . || true)
if [ "$table_count" != "1" ] || [ "$table_files" != "$TABLE" ]; then
  echo "[FAIL] payload-vmo: the RFC-0072 status table must be declared only in $TABLE, found:" >&2
  echo "$table_files" >&2
  fail=1
fi

# --- rule 5: the copying `pkg:/` path stays deleted -------------------------
# vfsd used to hold whole entries twice over: `packagefs_resolve` returned a
# `Vec` of the file and `FileHandle` kept another per open handle, both on a
# bump heap that never frees. That is the shape that made 22 of the system
# volume's 115 entries unreadable and fatal to packagefsd. The splice handler
# must forward the caller's VMO, never ask the namespace to open bytes for it.
if grep -qE 'namespace\.open\(' source/services/vfsd/src/splice_os.rs; then
  echo "[FAIL] payload-vmo: vfsd's splice handler opens entries again instead of forwarding the VMO" >&2
  fail=1
fi
# os-lite only: the host-side `std_server.rs` is a different backend with its
# own in-memory provider, and holding bytes there is what it is for.
if grep -nE '^\s*(pub\(crate\) )?bytes: Vec<u8>,' \
    source/services/vfsd/src/os_lite.rs source/services/vfsd/src/namespace.rs; then
  echo "[FAIL] payload-vmo: a vfsd os-lite handle/entry carries file bytes again (see above)" >&2
  fail=1
fi

if [ "$fail" == "0" ]; then
  echo "[PASS] payload-vmo: one header codec, one status table, no retired magic or private status space, no copying pkg:/ path"
fi
exit "$fail"
