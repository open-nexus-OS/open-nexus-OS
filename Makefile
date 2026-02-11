SHELL := /bin/sh
MODE ?= container
CONTAINER_TAG ?= open-nexus-os:dev
NIGHTLY ?= nightly-2025-01-15
CARGO_BIN ?= cargo
SMP ?= 2
HOST_RUSTFLAGS := --check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="host"
UID := $(shell id -u)
GID := $(shell id -g)
SELINUX_LABEL := $(shell command -v selinuxenabled >/dev/null 2>&1 && selinuxenabled && echo ":Z" || true)

.PHONY: initial-setup build test verify run pull clean
.PHONY: run-init-host test-init-host
.PHONY: dep-gate

initial-setup:
	@echo "==> Checking podman rootless support"
	@scripts/check-rootless.sh
	@echo "==> Checking QEMU build deps (for virtio-mmio modern patch)"
	@command -v ninja >/dev/null 2>&1 || echo "[warn] ninja not found (required for QEMU build)"
	@command -v meson >/dev/null 2>&1 || echo "[warn] meson not found (required for QEMU build)"
	@echo "==> Installing Rust targets"
	@rustup target add riscv64imac-unknown-none-elf
	@echo "==> Installing git hooks"
	@./scripts/fmt-clippy-deny.sh
	@echo "==> QEMU modern virtio-mmio patch: run ./tools/qemu/build-modern.sh"

build:
ifeq ($(MODE),container)
	@echo "==> Building workspace inside podman"
	@podman build -t $(CONTAINER_TAG) -f podman/Containerfile .
	@podman run --rm -t \
		--userns=keep-id -u $(UID):$(GID) \
		--entrypoint "" \
		-v "$(CURDIR)":/workspace$(SELINUX_LABEL) -w /workspace \
		-e CARGO_HOME=/workspace/.cargo \
		-e RUSTUP_HOME=/workspace/.rustup \
		-e CARGO_TARGET_DIR=/workspace/target \
		-e PATH=/workspace/.cargo/bin:/home/builder/.cargo/bin:/root/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
		$(CONTAINER_TAG) \
		sh -lc '\
		  echo "[1/2] host+os userspace build"; \
		  mkdir -p "$$RUSTUP_HOME" "$$CARGO_HOME"; \
		  rustup default stable; \
		  RUSTFLAGS="--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"host\"" $(CARGO_BIN) build --workspace --exclude neuron --exclude neuron-boot --exclude samgrd --exclude bundlemgrd --exclude identityd --exclude dsoftbusd --exclude dist-data --exclude clipboardd --exclude notifd --exclude resmgrd --exclude searchd --exclude settingsd --exclude time-syncd --exclude netstackd && \
		  echo "[1b/2] cross-compile OS services (riscv64)"; \
		  RUSTFLAGS="--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"os\"" $(CARGO_BIN) +$(NIGHTLY) build -p samgrd -p bundlemgrd -p dsoftbusd -p execd -p keystored -p netstackd -p packagefsd -p policyd -p vfsd --target riscv64imac-unknown-none-elf --no-default-features --features os-lite && \
		  echo "[1c/2] RFC-0009 dep-gate (OS graph)"; \
		  forbidden="parking_lot parking_lot_core getrandom"; \
		  services="dsoftbusd netstackd keystored policyd samgrd bundlemgrd packagefsd vfsd execd"; \
		  found=0; \
		  for svc in $$services; do \
		    tree_output=$$($(CARGO_BIN) +$(NIGHTLY) tree -p "$$svc" --target riscv64imac-unknown-none-elf --no-default-features --features os-lite 2>&1 || true); \
		    for f in $$forbidden; do \
		      echo "$$tree_output" | grep -qE "^[│├└ ]*$$f " && echo "[FAIL] $$svc pulled forbidden crate $$f" && found=1; \
		    done; \
		  done; \
		  test "$$found" -eq 0 && echo "[PASS] RFC-0009 dep-gate" || (echo "[FAIL] RFC-0009 dep-gate" && exit 1); \
		  RUSTFLAGS="--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"os\"" $(CARGO_BIN) +$(NIGHTLY) build -p nexus-init --lib --target riscv64imac-unknown-none-elf --no-default-features --features os-lite && \
	                  RUSTFLAGS="--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"os\"" $(CARGO_BIN) +$(NIGHTLY) build -p selftest-client --target riscv64imac-unknown-none-elf --no-default-features --features os-lite && \
	                  RUSTFLAGS="--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"os\"" $(CARGO_BIN) +$(NIGHTLY) build -p nexus-log --features sink-userspace --target riscv64imac-unknown-none-elf --release && \
	                  RUSTFLAGS="--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"os\"" $(CARGO_BIN) +$(NIGHTLY) build -p init-lite --target riscv64imac-unknown-none-elf --release && \
		  echo "[2/2] cross build kernel (riscv)"; \
		  rustup toolchain list | grep -q "$(NIGHTLY)" || rustup toolchain install "$(NIGHTLY)" --profile minimal; \
		  rustup component add rust-src --toolchain "$(NIGHTLY)"; \
		  rustup target add riscv64imac-unknown-none-elf --toolchain "$(NIGHTLY)"; \
		                  $(CARGO_BIN) +$(NIGHTLY) build \
		                    --target riscv64imac-unknown-none-elf -p neuron-boot --release'
else
	@echo "==> Building workspace on host"
	@RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="host"' cargo build --workspace --exclude neuron --exclude neuron-boot --exclude samgrd --exclude bundlemgrd --exclude identityd --exclude dsoftbusd --exclude dist-data --exclude clipboardd --exclude notifd --exclude resmgrd --exclude searchd --exclude settingsd --exclude time-syncd --exclude netstackd
	@echo "==> Cross-compiling OS services (riscv64)"
	@RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' cargo +$(NIGHTLY) build -p samgrd -p bundlemgrd -p dsoftbusd -p execd -p keystored -p netstackd -p packagefsd -p policyd -p vfsd --target riscv64imac-unknown-none-elf --no-default-features --features os-lite
	@$(MAKE) dep-gate
	@RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' cargo +$(NIGHTLY) build -p nexus-init --lib --target riscv64imac-unknown-none-elf --no-default-features --features os-lite
	@RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' cargo +$(NIGHTLY) build -p selftest-client --target riscv64imac-unknown-none-elf --no-default-features --features os-lite
	@RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' cargo +$(NIGHTLY) build -p nexus-log --features sink-userspace --target riscv64imac-unknown-none-elf --release
	@RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' cargo +$(NIGHTLY) build -p init-lite --target riscv64imac-unknown-none-elf --release
endif

test:
ifeq ($(MODE),container)
	@echo "==> Running host-first tests inside podman"
	@podman run --rm -t \
		--userns=keep-id -u $(UID):$(GID) \
		--entrypoint "" \
		-v "$(CURDIR)":/workspace$(SELINUX_LABEL) -w /workspace \
		-e CARGO_HOME=/workspace/.cargo \
		-e RUSTUP_HOME=/workspace/.rustup \
		-e CARGO_TARGET_DIR=/workspace/target \
		-e PATH=/workspace/.cargo/bin:/home/builder/.cargo/bin:/root/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
 		$(CONTAINER_TAG) \
 		sh -lc '\
 		  echo "[tests] host-first only (exclude neuron)"; \
		  if $(CARGO_BIN) nextest --version >/dev/null 2>&1; then \
		    RUSTFLAGS="--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"host\"" $(CARGO_BIN) nextest run --workspace --exclude neuron --exclude neuron-boot; \
		  else \
		    echo "[warn] cargo-nextest not found; falling back to cargo test"; \
		    RUSTFLAGS="--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"host\"" $(CARGO_BIN) test --workspace --exclude neuron --exclude neuron-boot; \
		  fi'
else
	@echo "==> Running host-first tests"
	@if cargo nextest --version >/dev/null 2>&1; then \
	  RUSTFLAGS='$(HOST_RUSTFLAGS)' cargo nextest run --workspace --exclude neuron --exclude neuron-boot; \
	else \
	  echo "[warn] cargo-nextest not found; falling back to cargo test"; \
	  RUSTFLAGS='$(HOST_RUSTFLAGS)' cargo test --workspace --exclude neuron --exclude neuron-boot; \
	fi
endif
	@echo "==> Running deterministic SMP ladder (default, SMP=$(SMP))"
	@SMP=$${SMP:-$(SMP)} REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=$${RUN_TIMEOUT:-190s} ./scripts/qemu-test.sh
	@SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=$${RUN_TIMEOUT:-190s} ./scripts/qemu-test.sh

verify:
	@echo "==> Running full verification (delegates to just workflow)"
	@command -v just >/dev/null 2>&1 || (echo "[error] just is required for 'make verify'" && exit 1)
	@just diag-host
	@just test-host
	@just test-e2e
	@just dep-gate
	@just diag-os
	@RUN_UNTIL_MARKER=1 just test-os
	@if [ "$${REQUIRE_SMP_VERIFY:-0}" = "1" ]; then \
	  SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=$${RUN_TIMEOUT:-90s} ./scripts/qemu-test.sh && \
	  SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=$${RUN_TIMEOUT:-90s} ./scripts/qemu-test.sh; \
	fi

run:
	@echo "==> Launching NEURON kernel under QEMU"
	@rustup toolchain list | grep -q "$(NIGHTLY)" || rustup toolchain install "$(NIGHTLY)" --profile minimal
	@rustup component add rust-src --toolchain "$(NIGHTLY)" >/dev/null 2>&1 || true
	@$(CARGO_BIN) +$(NIGHTLY) build --target riscv64imac-unknown-none-elf -p neuron-boot --release
	@run_until_marker=$${RUN_UNTIL_MARKER:-1}; \
	if [ "$$run_until_marker" != "0" ]; then \
	  echo "==> RUN_UNTIL_MARKER=$$run_until_marker: using scripts/qemu-test.sh (marker-driven early exit)"; \
	  SMP=$${SMP:-$(SMP)} RUN_TIMEOUT=$${RUN_TIMEOUT:-90s} RUN_UNTIL_MARKER=$$run_until_marker ./scripts/qemu-test.sh; \
	else \
	  UART_LOG=$${UART_LOG:-uart.log}; \
	  SMP=$${SMP:-$(SMP)} RUN_TIMEOUT=$${RUN_TIMEOUT:-30s} ./scripts/run-qemu-rv64.sh; \
	  status=$$?; \
	  if [ "$$status" = "124" ] && [ -f "$$UART_LOG" ] && grep -aFq "SELFTEST: end" "$$UART_LOG"; then \
	    echo "[warn] QEMU timed out, but UART log contains 'SELFTEST: end' (selftest completed)."; \
	    echo "[hint] For a truly green run, prefer: RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s make run"; \
	    exit 0; \
	  fi; \
	  exit $$status; \
	fi

dep-gate:
	@echo "==> RFC-0009 Dependency Hygiene Gate (Makefile)"
	@forbidden="parking_lot parking_lot_core getrandom"; \
	services="dsoftbusd netstackd keystored policyd samgrd bundlemgrd packagefsd vfsd execd"; \
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
	@rm -rf build
