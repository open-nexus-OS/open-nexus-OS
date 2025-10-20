#!/usr/bin/env bash
set -euo pipefail
RUN_UNTIL_MARKER=1 RUN_TIMEOUT=${RUN_TIMEOUT:-60s} just test-os
rg -n 'packagefsd: ready' uart.log >/dev/null
rg -n 'vfsd: ready'      uart.log >/dev/null
rg -n 'SELFTEST: vfs stat ok'  uart.log >/dev/null
rg -n 'SELFTEST: vfs read ok'  uart.log >/dev/null
rg -n 'SELFTEST: vfs ebadf ok' uart.log >/dev/null
echo "[postflight] userspace VFS proof OK"
