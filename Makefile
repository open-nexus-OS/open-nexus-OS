SHELL := /bin/bash
CONTAINER_TAG ?= open-nexus-os:dev
NIGHTLY ?= nightly-2025-01-15
SMP ?= 2
TARGET_DIR := target
export CARGO_TARGET_DIR := $(CURDIR)/$(TARGET_DIR)

# Canonical artifact paths that `make build` must produce and that
# `make test` / `make run` consume via NEXUS_SKIP_BUILD=1.
RV_TARGET := riscv64imac-unknown-none-elf
INIT_ELF := $(TARGET_DIR)/$(RV_TARGET)/release/init-lite
KERNEL_ELF := $(TARGET_DIR)/$(RV_TARGET)/release/neuron-boot
UID := $(shell id -u)
GID := $(shell id -g)
SELINUX_LABEL := $(shell command -v selinuxenabled >/dev/null 2>&1 && selinuxenabled && echo ":Z" || true)

.PHONY: initial-setup build test run pull clean
.PHONY: run-init-host test-init-host
.PHONY: dep-gate

initial-setup:
	@echo "==> Checking workspace location (must be under \$$HOME for rootless podman + cargo cache permissions)"
	@case "$(CURDIR)" in \
	  $$HOME/*) echo "[ok] workspace under \$$HOME ($(CURDIR))" ;; \
	  *) echo "[error] workspace is at $(CURDIR) which is NOT under \$$HOME ($$HOME)."; \
	     echo "[error] rootless podman + cargo target/ caches need user-owned paths."; \
	     echo "[error] move the checkout under \$$HOME (e.g. ~/open-nexus-OS) and re-run."; \
	     exit 1 ;; \
	esac
	@echo "==> Installing host packages (mirrors podman/Containerfile)"
	@./scripts/install-deps.sh $(if $(YES),--yes,)
	@echo "==> Checking podman rootless support"
	@scripts/check-rootless.sh
	@echo "==> Checking QEMU build deps (for virtio-mmio modern patch)"
	@command -v ninja >/dev/null 2>&1 || echo "[warn] ninja not found (required for QEMU build)"
	@command -v meson >/dev/null 2>&1 || echo "[warn] meson not found (required for QEMU build)"
	@echo "==> Installing Rust targets"
	@rustup target add riscv64imac-unknown-none-elf
	@echo "==> Running pre-commit gate (fmt + clippy + cargo-deny)"
	@# Note: this does NOT install a git hook. It just runs the same gate
	@# that a pre-commit hook would. To wire it as an actual hook, do:
	@#   ln -sf ../../scripts/fmt-clippy-deny.sh .git/hooks/pre-commit
	@./scripts/fmt-clippy-deny.sh
	@echo "==> QEMU modern virtio-mmio patch: run ./tools/qemu/build-modern.sh"

build:
	@echo "==> Building container image"
	@podman build --network=host -t $(CONTAINER_TAG) -f podman/Containerfile .
	@echo "==> Compiling workspace inside container"
	@podman run --rm -t \
		--network=host \
		--userns=keep-id -u $(UID):$(GID) \
		--entrypoint "" \
		-v "$(CURDIR)":/workspace$(SELINUX_LABEL) -w /workspace \
		-e CARGO_HOME=/workspace/.cargo \
		-e RUSTUP_HOME=/workspace/.rustup \
		-e CARGO_TARGET_DIR=/workspace/target \
		-e BUILD_TMPDIR_DEFAULT=/workspace/.tmp/build \
		-e PATH=/workspace/.cargo/bin:/home/builder/.cargo/bin:/root/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
		$(CONTAINER_TAG) \
		bash -lc '\
			mkdir -p "$$RUSTUP_HOME" "$$CARGO_HOME"; \
			rustup default stable; \
			echo "==> host workspace build"; \
			RUSTFLAGS="--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"host\"" cargo build --workspace --exclude neuron --exclude neuron-boot; \
			echo "==> cross-compile OS + kernel (scripts/build.sh)"; \
			./scripts/build.sh'

test:
	@echo "==> Running host tests inside container"
	@podman run --rm -t \
		--network=host \
		--userns=keep-id -u $(UID):$(GID) \
		--entrypoint "" \
		-v "$(CURDIR)":/workspace$(SELINUX_LABEL) -w /workspace \
		-e CARGO_HOME=/workspace/.cargo \
		-e RUSTUP_HOME=/workspace/.rustup \
		-e CARGO_TARGET_DIR=/workspace/target \
		-e PATH=/workspace/.cargo/bin:/home/builder/.cargo/bin:/root/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
		$(CONTAINER_TAG) \
		bash -lc '\
			mkdir -p "$$RUSTUP_HOME" "$$CARGO_HOME"; \
			rustup default stable; \
			if cargo nextest --version >/dev/null 2>&1; then \
			  RUSTFLAGS="--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"host\"" cargo nextest run --workspace --exclude neuron --exclude neuron-boot; \
			else \
			  RUSTFLAGS="--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"host\"" cargo test --workspace --exclude neuron --exclude neuron-boot; \
			fi'
	@echo "==> Running headless QEMU smoke (full service chain, no display)"
	@NEXUS_SKIP_BUILD=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=$${RUN_TIMEOUT:-120s} ./scripts/qemu-test.sh --profile=headless
	@echo "==> Running SMP ladder (SMP=2 strict + SMP=1 parity)"
	@NEXUS_SKIP_BUILD=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=$${RUN_TIMEOUT:-190s} ./scripts/qemu-test.sh --profile=smp
	@NEXUS_SKIP_BUILD=1 SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=$${RUN_TIMEOUT:-190s} ./scripts/qemu-test.sh --profile=smp
	@echo "==> Running DHCP smoke (network stack proof)"
	@NEXUS_SKIP_BUILD=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=$${RUN_TIMEOUT:-120s} ./scripts/qemu-test.sh --profile=dhcp

# Note: `make verify` was retired in favor of `just test-all`, which is the
# canonical aggregate gate (fmt-check + lint + deny + host tests + e2e +
# miri + arch-check + kernel build + ci-os-smp). The `make` spur stays
# self-contained (no `just` dependency) and limits itself to build/test/run.

run:
	@echo "==> Launching interactive session (uses 'make build' artifacts)"
	@NEXUS_SKIP_BUILD=1 \
	  NEXUS_DISPLAY_BOOTSTRAP=1 \
	  SMP=$${SMP:-$(SMP)} \
	  QEMU_SESSION_MODE=interactive \
	  QEMU_MARKER_LEVEL=full \
	  NEXUS_SELFTEST_MODE=interactive-full \
	  NEXUS_SELFTEST_PROFILE=none \
	  RUN_UNTIL_MARKER=0 \
	  RUN_TIMEOUT=$${RUN_TIMEOUT:-0} \
	  ./scripts/qemu-launcher.sh

dep-gate:
	@echo "==> RFC-0009 Dependency Hygiene Gate (Makefile)"
	@forbidden="parking_lot parking_lot_core getrandom"; \
	services="dsoftbusd netstackd keystored policyd samgrd bundlemgrd packagefsd vfsd execd timed metricsd hidrawd touchd inputd gpud windowd"; \
	found=0; \
	for svc in $$services; do \
	  tree_output=$$(cargo +$(NIGHTLY) tree -p "$$svc" --target riscv64imac-unknown-none-elf --no-default-features --features os-lite 2>&1 || true); \
	  for f in $$forbidden; do \
	    echo "$$tree_output" | grep -qE "^[│├└ ]*$$f " && echo "[FAIL] $$svc pulled forbidden crate $$f" && found=1; \
	  done; \
	done; \
	test "$$found" -eq 0 && echo "[PASS] RFC-0009 dep-gate" || (echo "[FAIL] RFC-0009 dep-gate" && exit 1)

run-init-host:
	@echo "==> Running host nexus-init (will exit on init: ready)"
	@RUN_TIMEOUT=$${RUN_TIMEOUT:-30s} ./scripts/host-init-test.sh

test-init-host:
	@echo "==> Host init test"
	@./scripts/host-init-test.sh

pull:
	@echo "==> Refreshing recipe sources"
	@find recipes -name recipe.toml -print | while read -r recipe; do \
		echo "Syncing $$recipe"; \
		grep '^\[source\]' -n "$$recipe" >/dev/null || true; \
	done

clean:
	@echo "==> Cleaning build artifacts"
	@cargo clean
	@rm -f build/blk.img build/blk-A.img build/blk-B.img
	@rm -f build/.qemu-blk.lock build/qemu.qmp build/.interactive-scene-ready
