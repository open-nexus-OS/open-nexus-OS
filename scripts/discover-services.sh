#!/usr/bin/env bash
# Auto-discover OS services from cargo metadata — the build-truth SSOT
# (TASK-0324 P0): every per-service build fact lives in the crate's manifest,
# and BOTH build paths (embedded init-lite table, system-volume bundles) resolve
# it through this script. There is no second place to put a cargo feature.
#
# Each service crate must have:
#   [package.metadata.nexus-service]
#   stack_pages = 0            # optional, default 0
#   features = ["os-lite"]     # optional, default ["os-lite"]
#   [package.metadata.nexus-service.feature_profiles]
#   gpu_mode = { virgl = ["virgl"] }   # optional: env GPU_MODE=virgl adds `virgl`
#
# Output modes:
#   --list                     service names, one per line
#   --build-args               "-p svc1 -p svc2 ..." for cargo build
#   --env-vars                 "INIT_LITE_SERVICE_SVC1_ELF=... INIT_LITE_SERVICE_SVC1_STACK_PAGES=..."
#   --dep-gate-list            "svc1 svc2 ..." for dep-gate check
#   --cargo-features <svc>     resolved feature list, comma-joined (e.g. os-lite,virgl)
#   --feature-key <svc>        the same list as an artifact-path key (e.g. os-lite+virgl)
#   --stack-pages <svc>        the service's stack_pages
#   --elf-path <svc>           build/services/<svc>/<feature-key>/payload.elf

set -euo pipefail

MODE="${1:---list}"
SVC_ARG="${2:-}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
RV_TARGET="riscv64imac-unknown-none-elf"

# Run cargo metadata, extract packages with nexus-service metadata
SERVICES=$(cargo metadata --format-version=1 --manifest-path "$ROOT/Cargo.toml" 2>/dev/null |   python3 -c "
import json, os, sys
data = json.load(sys.stdin)

# Services that are not yet ready for OS cross-compilation
# (pull in forbidden crates or fail no_std compilation)
OS_SKIP = {'identityd', 'debugsvc'}

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
    # Build-truth: the feature list is data, resolved here for EVERY consumer.
    features = list(meta.get('features', ['os-lite']))
    for env_key, table in (meta.get('feature_profiles') or {}).items():
        value = os.environ.get(env_key.upper())
        if value is not None and value in table:
            for f in table[value]:
                if f not in features:
                    features.append(f)
    svcs.append((name, stack, ','.join(features), kind))
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
    'settingsd',
    'updated',
    'timed',
    'packagefsd',
    'vfsd',
    'execd',
    'abilitymgr',
    'sessiond',
    'netstackd',
    'ingressd',
    'dsoftbusd',
    'hidrawd',
    'touchd',
    'gpud',
    'windowd',
    'inputd',
    'imed',
    'pinched',
    'selftest-client',
    'bootctld',
]
rank = {name: idx for idx, name in enumerate(ORDER)}
svcs.sort(key=lambda x: (rank.get(x[0], len(ORDER)), x[0]))
for name, stack, features, kind in svcs:
    print(f'{name} {stack} {features} {kind}')
")

if [ -z "$SERVICES" ]; then
    echo "[warn] no services found via cargo metadata" >&2
    exit 0
fi

# Boot-table rows only (payload crates are built, never spawned by init).
BOOT_SERVICES=$(echo "$SERVICES" | awk '$4 != "payload"')
# One row for the service named in $SVC_ARG (modes that take a service).
svc_row() {
    local row
    row=$(echo "$SERVICES" | awk -v s="$SVC_ARG" '$1 == s')
    if [ -z "$row" ]; then
        echo "[error] discover-services: unknown service '$SVC_ARG'" >&2
        exit 1
    fi
    echo "$row"
}
# Artifact path keyed by the resolved feature set: two feature sets of one
# service never share an ELF (the 2026-09-09 black screen: the embedded and
# the bundle build overwrote each other in target/…/release/gpud).
elf_path_for() {
    local name="$1" features="$2"
    printf "%s/build/services/%s/%s/payload.elf" "$ROOT" "$name" "${features//,/+}"
}

case "$MODE" in
    --list)
        echo "$BOOT_SERVICES" | while read name stack features kind; do echo "$name"; done
        ;;
    --build-args)
        echo "$BOOT_SERVICES" | while read name stack features kind; do printf " -p %s" "$name"; done
        echo
        ;;
    --env-vars)
        echo "$BOOT_SERVICES" | while read name stack features kind; do
            upper=$(echo "$name" | tr '[:lower:]' '[:upper:]' | tr '-' '_')
            printf "INIT_LITE_SERVICE_%s_ELF=%s " "$upper" "$(elf_path_for "$name" "$features")"
            if [ "$stack" -gt 0 ]; then
                printf "INIT_LITE_SERVICE_%s_STACK_PAGES=%s " "$upper" "$stack"
            fi
        done
        echo
        ;;
    --dep-gate-list)
        echo "$BOOT_SERVICES" | while read name stack features kind; do printf "%s " "$name"; done
        echo
        ;;
    --cargo-features)
        svc_row | awk '{print $3}'
        ;;
    --feature-key)
        svc_row | awk '{gsub(",", "+", $3); print $3}'
        ;;
    --stack-pages)
        svc_row | awk '{print $2}'
        ;;
    --elf-path)
        row=$(svc_row); elf_path_for "$(echo "$row" | awk '{print $1}')" "$(echo "$row" | awk '{print $3}')"; echo
        ;;
    *)
        echo "usage: $0 [--list|--build-args|--env-vars|--dep-gate-list|--cargo-features <svc>|--feature-key <svc>|--stack-pages <svc>|--elf-path <svc>]" >&2
        exit 1
        ;;
esac
