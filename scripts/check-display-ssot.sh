#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Display-mode SSOT gate (TASK-0324 P6-a, RFC-0093 §5, RFC-0098 C7). The VISIBLE
# display mode has ONE authority — gpud, which reads the lane's request from its own tree slot,
# applies ONE policy (`nexus_display_proto::resolve_display_mode`, clamped to the ONE declared
# `LAYOUT_MAX`) and grants the mode together with the framebuffer it owns. Before, windowd and
# inputd each grew a PROTOCOL to poll someone else for the answer, each with a silent 1280x800
# fallback on every failure path — a wrong mode could latch for a whole session unnoticed.
#
# Rules (non-comment code only):
#   1. The retired (polled) query protocols stay retired (windowd->gpud and inputd->windowd).
#      The mode now travels gpud -> windowd in the framebuffer grant, and windowd -> inputd
#      in ONE call over inputd's declared reply inbox — no retry, no default.
#   7. The framebuffer is gpud's (RFC-0098 C7): windowd allocates no VMO — a scanout
#      buffer must be made FOR the scanout device, and only gpud holds that capability.
#   2. No mode-retry machinery in windowd/inputd — the mode is read, not polled.
#   3. No `1280, 800` literal in windowd/inputd/gpud production code: the layout maximum
#      is declared once in `nexus-display-proto`. Test fixtures are exempt (they assert
#      against concrete geometry, which is their job).
#   5. The reveal DECISION carries no time term (RFC-0093 §5 gate: no `nsec()` in gpud's
#      reveal path) — `let should_reveal = …;` in gl_scanout.rs is evidence-only.
#   6. Readback goes through the probe RT only: no `virgl_transfer_from_host(` call in
#      gl_scanout.rs / service.rs (gl_probe.rs is the ONE readback authority).
#   4. The display-handoff heuristics stay deleted (RFC-0093 §5, P6-b/c): no present-ack
#      lease, no stall recovery, no first-handoff deadline, no reveal time caps, no pixel
#      probe. Acks are matched by seq; reveal is a handshake. Each of these existed only
#      because the protocol could not express the fact it guessed at.
#
# `--self-test` proves the scanner can FAIL, against fixtures in a temp dir — a gate that
# cannot fail is not a gate (see `just deadcode`, the recorded counter-example).
set -euo pipefail
cd "$(dirname "$0")/.."

PROD_PATHS=(source/services/windowd/src source/services/inputd/src source/drivers/gpud/src \
            source/services/systemui/src)

# The scanner is Python, not grep, for one reason: the layout-maximum literal also appears in
# `#[cfg(test)] mod tests` blocks INSIDE production files, which no path filter can see. Test
# code asserts against concrete geometry — that is its job — so it is skipped by BLOCK, and a
# comment naming a retired symbol is history, not a resurrection.
scan() {
    python3 - "$@" <<'PYEOF'
import re, sys, os

RETIRED = re.compile(
    r"\b(OP_GET_DISPLAY_MODE|encode_display_mode_reply|decode_display_mode_reply"
    r"|OP_GET_VISIBLE_MODE|encode_get_visible_mode|encode_visible_mode_reply"
    r"|decode_visible_mode_reply)\b")
# NB: no trailing \b — the real constant was `DISPLAY_MODE_RETRY_NS`, and `_` is a word
# character, so an anchored tail would have missed the very thing this rule exists for.
RETRY = re.compile(r"\bDISPLAY_MODE_RETRY")
HEURISTICS = re.compile(
    r"\b(PRESENT_ACK_LEASE_NS|LAST_ACK_NS|LEASE_REPORTED|present_lease_expired"
    r"|FIRST_HANDOFF_DEADLINE_NS|STALL_THRESHOLD_NS|REVEAL_FALLBACK_NS|REVEAL_HARD_CAP_NS"
    r"|plane0_has_content|reveal_content_since_ns)\b")
LITERAL = re.compile(r"\b1280\s*,\s*800\b")

def scan_file(path):
    out = []
    depth_stack = []          # brace depth where a cfg(test) block started
    depth = 0
    pending_cfg_test = False
    for n, raw in enumerate(open(path, encoding="utf-8", errors="replace"), 1):
        line = raw.rstrip("\n")
        stripped = line.strip()
        in_test = bool(depth_stack)
        if not (stripped.startswith("//") or stripped.startswith("/*") or stripped.startswith("*")):
            if not in_test:
                for rx in (RETIRED, RETRY, LITERAL, HEURISTICS):
                    if rx.search(line):
                        out.append(f"{path}:{n}:{stripped}")
                        break
            if pending_cfg_test and "{" in line:
                depth_stack.append(depth + line.count("{") - line.count("}"))
                pending_cfg_test = False
            elif re.match(r"#\[cfg\((all\()?test", stripped) or stripped == "#[cfg(test)]":
                pending_cfg_test = True
        depth += line.count("{") - line.count("}")
        while depth_stack and depth < depth_stack[-1]:
            depth_stack.pop()
    return out

hits = []
for root in sys.argv[1:]:
    for dirpath, _, names in os.walk(root):
        for name in names:
            if name.endswith(".rs") and not name.endswith("tests.rs"):
                hits += scan_file(os.path.join(dirpath, name))
print("\n".join(hits))
PYEOF
}

# Rule 5+6 run against a ROOT (real tree or the self-test fixtures).
reveal_and_readback_rules() {
    local root="$1" bad=0
    local gls; gls=$(find "$root" -name gl_scanout.rs | head -1)
    if [ -n "$gls" ]; then
        # the reveal decision statement, multi-line, must be free of time terms
        if awk '/let should_reveal =/{f=1} f{print} f&&/;/{exit}' "$gls" | grep -qE 'nsec\(|elapsed|_NS\b'; then
            echo "[FAIL] display-ssot: the reveal decision in gl_scanout.rs has a time term (RFC-0093 §5):" >&2
            awk '/let should_reveal =/{f=1} f{print} f&&/;/{exit}' "$gls" >&2
            bad=1
        fi
    fi
    local f
    for f in $(find "$root" \( -name gl_scanout.rs -o -name service.rs \)); do
        if grep -nE 'virgl_transfer_from_host\(' "$f" | grep -vE 'fn virgl_transfer_from_host' | grep -vE ':[0-9]+:[[:space:]]*//' >/dev/null; then
            echo "[FAIL] display-ssot: $f reads back outside gl_probe.rs (the ONE readback authority):" >&2
            grep -nE 'virgl_transfer_from_host\(' "$f" | grep -vE 'fn virgl_transfer_from_host' >&2
            bad=1
        fi
    done
    return $bad
}

# Rule 7 runs against a ROOT (windowd's source, or the self-test fixtures).
framebuffer_owner_rule() {
    local root="$1" hits
    hits=$(grep -rnE '\bvmo_create(_for|_contiguous)?[[:space:]]*\(' "$root" --include='*.rs' \
        | grep -vE ':[0-9]+:[[:space:]]*//' || true)
    if [ -n "$hits" ]; then
        echo "[FAIL] display-ssot: windowd allocates a VMO — the framebuffer is gpud's (RFC-0098 C7):" >&2
        printf '%s\n' "$hits" >&2
        return 1
    fi
    return 0
}

if [ "${1:-}" = "--self-test" ]; then
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    mkdir -p "$tmp/src"
    cat > "$tmp/src/fixture.rs" <<'FIX'
// A comment naming OP_GET_DISPLAY_MODE is history, not a violation.
const A: u8 = nexus_display_proto::OP_GET_DISPLAY_MODE;
const DISPLAY_MODE_RETRY_NS: u64 = 200_000_000;
const MAX: (u32, u32) = (1280, 800);
fn f() { let _ = decode_visible_mode_reply(&f); }
const REVEAL_HARD_CAP_NS: u64 = 1_200_000_000;

#[cfg(test)]
mod tests {
    // Test code asserts against concrete geometry: not a violation.
    fn t() { assert_eq!(size(), (1280, 800)); }
}
FIX
    n=$(scan "$tmp/src" | grep -c . || true)
    if [ "$n" -ne 5 ]; then
        echo "[FAIL] display-ssot: scanner self-test expected 5 hits, got $n" >&2
        scan "$tmp/src" >&2
        exit 1
    fi
    mkdir -p "$tmp/bad" "$tmp/good"
    printf 'let should_reveal = self.uploaded || elapsed > REVEAL_CAP_NS;\n' > "$tmp/bad/gl_scanout.rs"
    printf 'fn x(&mut self) { self.virgl_transfer_from_host(self.rt_front_res(), 0, 0, 1, 1, 4); }\n' > "$tmp/bad/service.rs"
    if reveal_and_readback_rules "$tmp/bad" 2>/dev/null; then
        echo "[FAIL] display-ssot: reveal/readback rules did not catch the fixtures" >&2
        exit 1
    fi
    printf 'let should_reveal = self.wallpaper_from_vmo_uploaded || self.reveal_requested;\n' > "$tmp/good/gl_scanout.rs"
    printf 'pub(crate) fn virgl_transfer_from_host(&mut self) {}\n' > "$tmp/good/service.rs"
    if ! reveal_and_readback_rules "$tmp/good"; then
        echo "[FAIL] display-ssot: reveal/readback rules reject a clean tree" >&2
        exit 1
    fi
    mkdir -p "$tmp/wbad" "$tmp/wgood"
    printf 'fn fb() { let h = vmo_create(len); }\n' > "$tmp/wbad/mod.rs"
    printf '// windowd no longer calls vmo_create( — the framebuffer is granted.\nfn fb(g: Grant) { use_it(g.framebuffer); }\n' > "$tmp/wgood/mod.rs"
    if framebuffer_owner_rule "$tmp/wbad" 2>/dev/null; then
        echo "[FAIL] display-ssot: the framebuffer-owner rule did not catch the fixture" >&2
        exit 1
    fi
    if ! framebuffer_owner_rule "$tmp/wgood"; then
        echo "[FAIL] display-ssot: the framebuffer-owner rule rejects a clean tree" >&2
        exit 1
    fi
    echo "[ok]   display-ssot: scanner self-test passed (5 shapes caught, test block skipped, reveal/readback and framebuffer-owner rules fail on fixtures)"
    exit 0
fi

violations=$(scan "${PROD_PATHS[@]}" | grep . || true)
if [ -n "$violations" ]; then
    echo "[FAIL] display-ssot: the display mode has one authority and one policy (RFC-0098 C7):" >&2
    printf '%s\n' "$violations" >&2
    echo "       gpud decides it (resolve_display_mode at probe) and grants it with the" >&2
    echo "       framebuffer; inputd asks windowd once. Nobody polls, nobody defaults." >&2
    exit 1
fi
reveal_and_readback_rules source/drivers/gpud/src || exit 1
framebuffer_owner_rule source/services/windowd/src || exit 1
echo "[PASS] display-ssot: one mode authority, one clamp policy, gpud owns the framebuffer, no retired query protocol, no resurrected handoff heuristic, evidence-only reveal, probe-only readback"
