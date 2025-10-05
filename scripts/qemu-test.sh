#!/usr/bin/env bash
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.
#
# Integration runner that enables RUN_UNTIL_MARKER by default and verifies the
# UART stream exposes the success markers emitted by the NEURON self-tests.
# The same log trimming safeguards as the standard runner apply.

set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd -- "${SCRIPT_DIR}/.." && pwd)
cd "$REPO_ROOT"

export RUN_UNTIL_MARKER=${RUN_UNTIL_MARKER:-1}
export RUN_TIMEOUT=${RUN_TIMEOUT:-30s}
export QEMU_LOG_MAX=${QEMU_LOG_MAX:-52428800}
export UART_LOG_MAX=${UART_LOG_MAX:-10485760}
export UART_LOG=${UART_LOG:-uart.log}

"$SCRIPT_DIR/run-qemu-rv64.sh" "$@"

if [[ "$RUN_UNTIL_MARKER" == "1" ]]; then
  if ! grep -qE 'SELFTEST: end|samgrd: ready|bundlemgrd: ready' "$UART_LOG"; then
    echo "[error] Expected success markers were not observed in $UART_LOG." >&2
    exit 1
  fi
fi
