#!/usr/bin/env bash
# SessionStart hook (RFC-0115): put the planning state into the session's context — the active
# milestone, this week's sprint and the work in progress — so a new session starts oriented in a
# few hundred tokens instead of reading ledgers. Uses the built `nx`; never builds (fast start).
root="${CLAUDE_PROJECT_DIR:-$(pwd)}"
for bin in "$root/target/debug/nx" "$root/target/release/nx"; do
  if [ -x "$bin" ]; then
    echo "## Planning state (nx ledger brief)"
    (cd "$root" && "$bin" ledger brief 2>/dev/null) && exit 0
  fi
done
echo "Planning state: run \`just brief\` (nx is not built yet)."
