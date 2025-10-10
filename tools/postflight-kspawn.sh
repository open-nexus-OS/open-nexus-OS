#!/usr/bin/env bash
set -euo pipefail
RUN_UNTIL_MARKER=1 RUN_TIMEOUT=${RUN_TIMEOUT:-45s} just test-os
rg -n 'KSELFTEST: spawn ok' uart.log >/dev/null
echo "[postflight-kspawn] OK"
