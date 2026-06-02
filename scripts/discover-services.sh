#!/usr/bin/env bash
# Auto-discover OS services from cargo metadata.
# 
# Each service crate under source/services/ must have:
#   [package.metadata.nexus-service]
#   stack_pages = 0    # optional, default 0
#
# Output modes:
#   --list             service names, one per line
#   --build-args       "-p svc1 -p svc2 ..." for cargo build
#   --env-vars         "INIT_LITE_SERVICE_SVC1_ELF=... INIT_LITE_SERVICE_SVC1_STACK_PAGES=..."  
#   --dep-gate-list    "svc1 svc2 ..." for dep-gate check

set -euo pipefail

MODE="${1:---list}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
RV_TARGET="riscv64imac-unknown-none-elf"

# Run cargo metadata, extract packages with nexus-service metadata
SERVICES=$(cargo metadata --format-version=1 --manifest-path "$ROOT/Cargo.toml" 2>/dev/null |   python3 -c "
import json, sys
data = json.load(sys.stdin)

# Services that are not yet ready for OS cross-compilation
# (pull in forbidden crates or fail no_std compilation)
OS_SKIP = {'identityd', 'debugsvc', 'virtioblkd'}

svcs = []
for pkg in data.get('packages', []):
    metadata = pkg.get('metadata')
    if metadata is None:
        continue
    meta = metadata.get('nexus-service')
    if meta is None:
        continue
    name = pkg['name']
    if name in OS_SKIP:
        continue
    kind = meta.get('kind', 'service')
    # Libraries are dependencies, not standalone services to build/embed
    if kind == 'library':
        continue
    stack = meta.get('stack_pages', 0)
    svcs.append((name, stack))
# Deterministic boot order for init-lite. Keep this as the single service-order
# policy so harnesses do not each carry their own stale service list.
ORDER = [
    'keystored',
    'rngd',
    'policyd',
    'logd',
    'metricsd',
    'samgrd',
    'bundlemgrd',
    'statefsd',
    'updated',
    'timed',
    'packagefsd',
    'vfsd',
    'execd',
    'netstackd',
    'dsoftbusd',
    'hidrawd',
    'touchd',
    'gpud',
    'windowd',
    'inputd',
    'selftest-client',
]
rank = {name: idx for idx, name in enumerate(ORDER)}
svcs.sort(key=lambda x: (rank.get(x[0], len(ORDER)), x[0]))
for name, stack in svcs:
    print(f'{name} {stack}')
")

if [ -z "$SERVICES" ]; then
    echo "[warn] no services found via cargo metadata" >&2
    exit 0
fi

case "$MODE" in
    --list)
        echo "$SERVICES" | while read name stack; do echo "$name"; done
        ;;
    --build-args)
        args=""
        echo "$SERVICES" | while read name stack; do
            args="$args -p $name"
        done
        # output all on one line
        echo "$SERVICES" | while read name stack; do printf " -p %s" "$name"; done
        echo
        ;;
    --env-vars)
        echo "$SERVICES" | while read name stack; do
            upper=$(echo "$name" | tr '[:lower:]' '[:upper:]' | tr '-' '_')
            printf "INIT_LITE_SERVICE_%s_ELF=%s/target/%s/release/%s " "$upper" "$ROOT" "$RV_TARGET" "$name"
            if [ "$stack" -gt 0 ]; then
                printf "INIT_LITE_SERVICE_%s_STACK_PAGES=%s " "$upper" "$stack"
            fi
        done
        echo
        ;;
    --dep-gate-list)
        echo "$SERVICES" | while read name stack; do printf "%s " "$name"; done
        echo
        ;;
    *)
        echo "usage: $0 [--list|--build-args|--env-vars|--dep-gate-list]" >&2
        exit 1
        ;;
esac
