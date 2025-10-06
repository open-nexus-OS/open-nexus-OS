#!/usr/bin/env bash
set -euo pipefail
rg -n --no-heading -g '!*target*' -e '#\[allow\((dead_code|unused)[^)]*\)\]' || true
echo "--- allowlist ---"
test -f config/deadcode.allow && cat config/deadcode.allow || true
# Fail if any allow(...) not whitelisted (format: <path>:<line> # until:YYYY-MM-DD reason)
if rg -n -g '!*target*' -e '#\[allow\((dead_code|unused)[^)]*\)\]' | \
   grep -vFf <(sed -E 's/#.*$//' config/deadcode.allow 2>/dev/null || true) | \
   grep .; then
  echo "[deadcode] unapproved allow(...) found"; exit 1
fi
# Fail expired allowlist entries
if test -f config/deadcode.allow; then
  awk -F'until:' '/until:/{print $2}' config/deadcode.allow | while read d; do
    [ -z "$d" ] && continue
    python3 - <<PY || exit 1
from datetime import date
assert date.today().isoformat() <= "${d}", "deadcode allow expired: ${d}"
PY
  done
fi

