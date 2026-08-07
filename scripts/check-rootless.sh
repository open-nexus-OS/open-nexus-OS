#!/bin/sh
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Assert rootless podman, which the Makefile's default container spur
#          (`make build` / `make test` with MODE=container) requires. Failing
#          here with an actionable message is the point — the same problem
#          surfaces deep inside a `podman run` as a permission error otherwise.
# OWNERS: @tools-team
# STATUS: Functional
set -eu

user="$(id -un)"

if ! command -v podman >/dev/null 2>&1; then
  echo "[error] podman is not installed." >&2
  echo "        fix: scripts/install-deps.sh" >&2
  echo "        or:  make build MODE=host   (skips the container spur entirely)" >&2
  exit 1
fi

if podman info --format '{{.Host.Security.Rootless}}' 2>/dev/null | grep -q true; then
  echo "Podman rootless environment detected."
  exit 0
fi

echo "[error] podman is not running rootless (required for the container spur)." >&2
echo "        1. install the setuid helpers: uidmap (Debian/Ubuntu),"          >&2
echo "           shadow-utils (Fedora), shadow (Arch)"                          >&2
if ! grep -q "^${user}:" /etc/subuid 2>/dev/null; then
  echo "        2. no /etc/subuid entry for ${user}:"                           >&2
  echo "           sudo usermod --add-subuids 100000-165535 \\"                 >&2
  echo "                        --add-subgids 100000-165535 ${user}"            >&2
else
  echo "        2. /etc/subuid entry for ${user} is present"                    >&2
fi
echo "        3. podman system migrate"                                         >&2
echo "        Alternative: make build MODE=host  (no container at all)"         >&2
exit 1
