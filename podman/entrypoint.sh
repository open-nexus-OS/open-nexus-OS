#!/bin/sh
set -eu

if [ "${1:-}" = "" ]; then
  exec make build MODE=host
fi

exec "$@"
