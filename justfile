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

set shell := ["bash", "-cu"]

# Boot NEURON in QEMU with bounded runtime and log trimming.
qemu RUN_TIMEOUT="30s" *ARGS:
    RUN_TIMEOUT={{RUN_TIMEOUT}} scripts/run-qemu-rv64.sh {{ARGS}}

# Execute NEURON smoke tests and require UART success markers.
test-os RUN_TIMEOUT="30s" *ARGS:
    RUN_TIMEOUT={{RUN_TIMEOUT}} RUN_UNTIL_MARKER=1 scripts/qemu-test.sh {{ARGS}}
