// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Chain-Test: Display Bootstrap (GPU-only architecture, RFC-0059 Phase 6)
//! OWNERS: @tools-team
//!
//! Verifiziert die GPU-only Kette:
//!   gpud start → virtio-gpu probe → ready
//!   windowd start → vmo_create → compose → present → gpud scanout handoff
//!
//! Ersetzt fbdevd/ramfb-basierte Chain-Tests (TASK-0062 Phase 6 GPU-only migration).

#[cfg(test)]
mod tests {
    use nx::chain::contract::{GpudContract, WindowdContract};
    use nx::chain::hop::ms;
    use nx::chain::{ChainRunner, ChainStatus};

    /// Chain: gpud → windowd → first frame (GPU-only self-bootstrap)
    ///
    /// Real markers observed from UART (visible-bootstrap run):
    ///   1. gpud: virtio-gpu probed       (gpud mapped MMIO, verified device id)
    ///   2. gpud: ready                   (gpud probe complete, IPC-ready)
    ///   3. windowd: runtime init ok      (Runtime initialized)
    ///   4. windowd: ready (w=1280,h=800,hz=120)
    ///   5. windowd: backend=gpu          (windowd creates own framebuffer VMO)
    ///   6. display: bootstrap on         (bootsplash starts)
    ///   7. display: mode 1280x800 argb8888
    ///   8. windowd: backend=visible      (runtime marks visible)
    ///   9. windowd: compose ready        (first frame composed)
    ///  10. windowd: present visible ok   (frame presented)
    ///  11. display: first scanout ok     (scanout confirmed)
    ///  12. systemui: first frame visible
    ///  13. SELFTEST: ui v2 present ok    (observer confirms)
    ///  14. SELFTEST: ui visible present ok
    ///  15. SELFTEST: ui visible input ok
    ///  16. SELFTEST: ui visible wheel ok
    ///  17. SELFTEST: ui v5 transition ok
    #[tokio::test]
    async fn chain_gpu_display_bootstrap() {
        let mut runner = ChainRunner::new("gpu-display-bootstrap");

        runner.register(Box::new(GpudContract::probe_only()));
        runner.register(Box::new(WindowdContract::visible_bootstrap(1280, 800)));

        // gpud markers
        runner
            .expect_marker("gpud: virtio-gpu probed", ms(500))
            .describe("gpud mapped virtio-gpu MMIO and verified device id");
        runner
            .expect_marker("gpud: ready", ms(300))
            .after(0)
            .describe("gpud probe complete, IPC-ready");

        // windowd markers
        runner
            .expect_marker("windowd: runtime init ok", ms(500))
            .describe("windowd runtime initialized");
        runner
            .expect_marker("windowd: ready (w=1280, h=800, hz=120)", ms(1000))
            .after(1)
            .describe("windowd ready with wallpaper");
        runner
            .expect_marker("windowd: backend=gpu", ms(200))
            .after(2)
            .describe("windowd creates own framebuffer VMO");
        runner
            .expect_marker("display: bootstrap on", ms(200))
            .after(3)
            .describe("bootsplash start");
        runner.expect_marker("display: mode 1280x800 argb8888", ms(100)).after(3);
        runner.expect_marker("windowd: backend=visible", ms(200)).after(4);
        runner
            .expect_marker("windowd: compose ready", ms(500))
            .after(5)
            .describe("first frame composed");
        runner
            .expect_marker("windowd: present visible ok", ms(300))
            .after(6)
            .describe("frame presented");
        runner
            .expect_marker("display: first scanout ok", ms(500))
            .after(7)
            .describe("scanout confirmed");
        runner.expect_marker("systemui: first frame visible", ms(200)).after(8);
        runner
            .expect_marker("SELFTEST: ui v2 present ok", ms(300))
            .after(9)
            .describe("observer confirms visible present");
        runner
            .expect_marker("SELFTEST: ui visible present ok", ms(300))
            .after(12)
            .describe("visible present summary");
        runner
            .expect_marker("SELFTEST: ui visible input ok", ms(300))
            .after(13)
            .describe("visible input summary");
        runner
            .expect_marker("SELFTEST: ui visible wheel ok", ms(300))
            .after(14)
            .describe("visible wheel summary");
        runner
            .expect_marker("SELFTEST: ui v5 transition ok", ms(300))
            .after(15)
            .describe("animation transition summary");

        let report = runner.run().await;
        if report.status != ChainStatus::Passed {
            eprintln!("{}", report.diagnostic());
        }
        assert_eq!(report.status, ChainStatus::Passed);
    }

    /// Chain: windowd starts without gpud (headless, GPU-unavailable fallback)
    #[tokio::test]
    async fn chain_windowd_starts_without_gpud() {
        let mut runner = ChainRunner::new("windowd-headless");

        // Only windowd — no gpud
        runner.register(Box::new(WindowdContract::headless()));

        runner.expect_marker("windowd: runtime init ok", ms(500));
        runner.expect_marker("windowd: ready (w=1280, h=800, hz=60)", ms(500));

        let report = runner.run().await;
        assert_eq!(report.status, ChainStatus::Passed);

        // No GPU-specific markers in headless mode
        assert!(report.hops.iter().all(|h| {
            !h.marker.contains("backend=gpu")
                && !h.marker.contains("gpud: ready")
                && !h.marker.contains("gpud: virtio-gpu")
        }));
    }
}
