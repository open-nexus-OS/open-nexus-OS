SHELL := /bin/bash
CONTAINER_TAG ?= open-nexus-os:dev
MODE ?= container
NIGHTLY ?= nightly-2025-01-15
SMP ?= 2
TARGET_DIR := target
export CARGO_TARGET_DIR := $(CURDIR)/$(TARGET_DIR)

# rustup installs its proxies into ~/.cargo/bin and wires that into the shell
# PROFILE — which an already-running shell has not read. Without this, the very
# `make initial-setup` that just installed rustup would then report it missing,
# and so would every `make build` until the user opened a new terminal.
export PATH := $(HOME)/.cargo/bin:$(PATH)

# Canonical artifact paths that `make build` must produce and that
# `make test` / `make run` consume via NEXUS_SKIP_BUILD=1.
RV_TARGET := riscv64imac-unknown-none-elf
INIT_ELF := $(TARGET_DIR)/$(RV_TARGET)/release/init-lite
KERNEL_ELF := $(TARGET_DIR)/$(RV_TARGET)/release/neuron-boot
UID := $(shell id -u)
GID := $(shell id -g)
SELINUX_LABEL := $(shell command -v selinuxenabled >/dev/null 2>&1 && selinuxenabled && echo ":Z" || true)

.PHONY: initial-setup doctor build test run pull clean
.PHONY: run-init-host test-init-host
.PHONY: dep-gate

# One-command bootstrap for a fresh Ubuntu/Debian, Fedora or Arch box that has
# nothing but git + curl. Everything it installs is verified afterwards by
# `make doctor`, which checks CAPABILITIES rather than package names — the only
# post-condition that means the same thing on all three distros.
#
# Flags:
#   YES=1      non-interactive (no sudo prompt, no y/N)
#   GUI=0      skip GTK/EGL/virgl — headless only, `just start` will not work
#   PODMAN=0   skip the rootless-podman checks (use `make build MODE=host`)
initial-setup:
	@echo "==> [1/6] Checking workspace location"
	@# Rootless podman maps this tree into a user namespace and cargo writes
	@# target/ as the invoking user: both need a user-owned path under $$HOME.
	@./scripts/check-deps.sh --workspace-only
	@echo "==> [2/6] Installing host packages"
	@./scripts/install-deps.sh $(if $(YES),--yes,) $(if $(filter 0,$(GUI)),--no-gui,)
	@echo "==> [3/6] Fetching declared build inputs (submodules + pinned fonts)"
	@./scripts/fetch-inputs.sh
	@echo "==> [4/6] Checking podman rootless support"
ifeq ($(PODMAN),0)
	@echo "[skip] PODMAN=0 — use 'make build MODE=host'"
else
	@./scripts/check-rootless.sh
endif
	@echo "==> [5/6] Wiring the pre-commit gate as a git hook"
	@if [ -e .git/hooks/pre-commit ]; then \
	  echo "[skip] .git/hooks/pre-commit already exists — leaving it alone"; \
	else \
	  ln -sf ../../scripts/fmt-clippy-deny.sh .git/hooks/pre-commit && \
	  echo "[ok] .git/hooks/pre-commit -> scripts/fmt-clippy-deny.sh"; \
	fi
	@echo "==> [6/6] Verifying the result"
	@GUI=$(if $(filter 0,$(GUI)),0,1) PODMAN=$(if $(filter 0,$(PODMAN)),0,1) ./scripts/check-deps.sh
	@echo ""
	@echo "Optional: ./tools/qemu/build-modern.sh builds a QEMU with force-modern"
	@echo "virtio-mmio defaults. Not required — the canonical harness passes"
	@echo "-global virtio-mmio.force-legacy=off itself."

# Verify this host can build, test and run the OS. No sudo, seconds, honest.
doctor:
	@./scripts/check-deps.sh

build:
ifeq ($(MODE),host)
	@echo "==> host workspace build (MODE=host)"
	@RUSTFLAGS="--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"host\"" \
		cargo build --workspace --exclude neuron --exclude neuron-boot
	@echo "==> cross-compile OS + kernel (scripts/build.sh)"
	@./scripts/build.sh
else
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
endif

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
	@echo "==> Running SMP ladder (profile smp = 2 harts strict + profile smp1 = 1-hart parity)"
	@NEXUS_SKIP_BUILD=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=$${RUN_TIMEOUT:-190s} ./scripts/qemu-test.sh --profile=smp
	@NEXUS_SKIP_BUILD=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=$${RUN_TIMEOUT:-190s} ./scripts/qemu-test.sh --profile=smp1
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
	@echo "==> RFC-0009 Dependency Hygiene Gate (Makefile; list = config/os-services.txt)"
	@forbidden="parking_lot parking_lot_core getrandom"; \
	services="$$(grep -v '^#' config/os-services.txt | tr '\n' ' ')"; \
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
