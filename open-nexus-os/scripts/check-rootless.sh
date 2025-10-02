#!/bin/sh
set -eu

if podman info --format '{{.Host.Security.Rootless}}' 2>/dev/null | grep -q true; then
  echo "Podman rootless environment detected."
  exit 0
fi

echo "Podman rootless mode is required." >&2
exit 1
