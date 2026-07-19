#!/usr/bin/env python3
"""PreToolUse guard — require explicit user approval for edits to CLAUDE.md protection zones.

Reads the `<zone ... access="approval">` paths from CLAUDE.md's `<protection_zones>` block
(the SSOT; the parseable attribute form exists exactly for this) and, when an Edit/Write/
MultiEdit/NotebookEdit targets a matching path, returns permissionDecision:"ask" so Claude
Code prompts the user to confirm. Non-protected edits pass through silently.

Design notes:
- The rule is "modifying needs EXPLICIT user approval", not "never modify" — so this asks,
  it does not hard-deny. Approved kernel/lib edits still happen, just never silently.
- Fail-open: any error → allow (exit 0). This is a defense-in-depth layer; the prose rule in
  CLAUDE.md remains the backstop, and a hook bug must never break normal editing.
- No `jq` dependency (not installed): pure Python stdlib.
- Coverage is the file-edit tools only. Writes via the Bash tool (`sed -i`, `>`, `tee`) are a
  known gap — reads are explicitly allowed, and reliably classifying arbitrary shell as
  read-vs-write is not worth the false positives.
"""
import sys
import os
import json
import re

FALLBACK_ZONES = [
    "source/kernel/**", "source/libs/**", "Cargo.toml", "Makefile",
    "scripts/**", "config/**", "recipes/meta/**", "docs/rfcs/**",
]
EDIT_TOOLS = ("Edit", "Write", "MultiEdit", "NotebookEdit")


def project_root(payload):
    root = os.environ.get("CLAUDE_PROJECT_DIR") or payload.get("cwd") or os.getcwd()
    d = root
    for _ in range(8):
        if os.path.isfile(os.path.join(d, "CLAUDE.md")):
            return d
        parent = os.path.dirname(d)
        if parent == d:
            break
        d = parent
    return root


def load_zones(root):
    try:
        with open(os.path.join(root, "CLAUDE.md"), encoding="utf-8") as f:
            text = f.read()
    except OSError:
        return FALLBACK_ZONES
    block = re.search(r"<protection_zones>(.*?)</protection_zones>", text, re.S)
    if not block:
        return FALLBACK_ZONES
    zones = []
    for zm in re.finditer(r'<zone\s+path="([^"]+)"[^>]*?access="([^"]+)"', block.group(1)):
        if zm.group(2) == "approval":
            zones.append(zm.group(1))
    return zones or FALLBACK_ZONES


def matches(rel, zone):
    rel = rel.replace("\\", "/")
    z = zone.replace("\\", "/")
    if z.endswith("/**"):
        pre = z[:-3]
        return rel == pre or rel.startswith(pre + "/")
    if z.endswith("/*"):
        pre = z[:-2]
        return rel == pre or rel.startswith(pre + "/")
    return rel == z


def main():
    payload = json.loads(sys.stdin.read())
    if payload.get("tool_name") not in EDIT_TOOLS:
        return
    fp = (payload.get("tool_input") or {}).get("file_path")
    if not fp:
        return
    root = project_root(payload)
    rel = os.path.relpath(os.path.abspath(fp), root).replace("\\", "/")
    if rel.startswith(".."):
        return  # outside the repo — not our concern
    for z in load_zones(root):
        if matches(rel, z):
            reason = (
                f"\U0001f6e1 {rel} is in a CLAUDE.md protection zone "
                f'(<zone path="{z}" access="approval">). Per the <protection_zones> rule this '
                f"edit needs EXPLICIT user approval in the current session. Confirm the user "
                f"approved editing this path before proceeding."
            )
            print(json.dumps({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "ask",
                    "permissionDecisionReason": reason,
                }
            }))
            return
    # not protected -> silent allow


if __name__ == "__main__":
    try:
        main()
    except Exception as e:  # fail-open: never break editing on a hook bug
        sys.stderr.write(f"protect-zones hook error (failing open): {e}\n")
    sys.exit(0)
