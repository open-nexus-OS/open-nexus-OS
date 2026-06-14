// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Chain-Test: GPU-Display bootstrap path
//! OWNERS: @tools-team
//!
//! Verifies: gpud probe → windowd compositor → first frame

#[cfg(test)]
mod tests {
    use nx::chain::contract::{GpudContract, WindowdContract};
    use nx::chain::hop::ms;
    use nx::chain::{ChainRunner, ChainStatus};

    /// Chain: gpud probes and windowd composes first frame
    #[tokio::test]
    async fn chain_gpu_scanout_success() {
        let mut runner = ChainRunner::new("gpu-scanout");

        runner.register(Box::new(GpudContract::probe_only()));
        runner.register(Box::new(WindowdContract::visible_bootstrap(1280, 800)));

        runner
            .expect_marker("gpud: virtio-gpu probed", ms(500))
            .describe("gpud probed virtio-gpu MMIO device");
        runner.expect_marker("gpud: ready", ms(200)).after(0);
        runner
            .expect_marker("windowd: ready (w=1280, h=800, hz=120)", ms(1000))
            .describe("windowd runtime ready");
        runner
            .expect_marker("display: first scanout ok", ms(500))
            .after(1)
            .describe("first frame scanout confirmed");

        let report = runner.run().await;
        if report.status != ChainStatus::Passed {
            eprintln!("{}", report.diagnostic());
        }
        assert_eq!(report.status, ChainStatus::Passed);
    }

    /// Chain: windowd GPU handoff path
    #[tokio::test]
    async fn chain_gpu_windowd_handoff() {
        let mut runner = ChainRunner::new("gpu-windowd-handoff");

        runner.register(Box::new(GpudContract::probe_only()));
        runner.register(Box::new(WindowdContract::visible_bootstrap(1280, 800)));

        runner
            .expect_marker("windowd: backend=gpu", ms(500))
            .describe("windowd creates framebuffer VMO");
        runner.expect_marker("windowd: backend=visible", ms(200)).after(0);
        runner
            .expect_marker("SELFTEST: ui v2 present ok", ms(300))
            .after(1)
            .describe("observer confirms present");
        runner
            .expect_marker("SELFTEST: ui visible input ok", ms(300))
            .after(2)
            .describe("observer confirms visible input path");
        runner
            .expect_marker("windowd: live transition ok", ms(300))
            .after(3)
            .describe("animation transition is live");
        runner
            .expect_marker("SELFTEST: ui v5 transition ok", ms(300))
            .after(4)
            .describe("observer confirms animation summary");

        let report = runner.run().await;
        if report.status != ChainStatus::Passed {
            eprintln!("{}", report.diagnostic());
        }
        assert_eq!(report.status, ChainStatus::Passed);
    }

    /// Chain: a single frame's journey through gpud's present chain. Mirrors the
    /// real GPUD_CHAIN_* hop markers (source/drivers/gpud/src/markers.rs); on a
    /// real run the last hop printed is the last stage reached, and the gpud
    /// diagnostic names the exact sub-stage on failure. This spec pins the order.
    #[tokio::test]
    async fn chain_gpu_present_hops_in_order() {
        let mut runner = ChainRunner::new("gpu-present-hops");

        runner.register(Box::new(GpudContract::with_handoff_and_cursor()));
        runner.register(Box::new(WindowdContract::visible_bootstrap(1280, 800)));

        runner
            .expect_marker("gpud: chain G1 recv present-damage", ms(1000))
            .describe("G1: gpud received OP_PRESENT_DAMAGE from windowd");
        runner
            .expect_marker("gpud: chain G2 parse ok", ms(200))
            .after(0)
            .describe("G2: command buffer deserialized (reload_from)");
        runner
            .expect_marker("gpud: chain G3 exec ok (commands applied)", ms(200))
            .after(1)
            .describe("G3: present_committed executed every command");
        runner
            .expect_marker("gpud: chain G4 scanout ok (frame presented)", ms(200))
            .after(2)
            .describe("G4: frame transferred + flushed to the scanout");

        let report = runner.run().await;
        if report.status != ChainStatus::Passed {
            eprintln!("{}", report.diagnostic());
        }
        assert_eq!(report.status, ChainStatus::Passed);
    }
}
