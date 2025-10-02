#!/bin/sh
set -eu

# Set PATH to include cargo
export PATH=/home/builder/.cargo/bin:/root/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

if [ "${1:-}" = "" ]; then
  exec make build MODE=host
else
  exec "$@"
fi
