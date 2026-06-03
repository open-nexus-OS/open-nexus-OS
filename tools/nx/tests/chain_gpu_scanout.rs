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
        runner
            .expect_marker("gpud: ready", ms(200))
            .after(0);
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
        runner
            .expect_marker("windowd: backend=visible", ms(200))
            .after(0);
        runner
            .expect_marker("SELFTEST: ui v2 present ok", ms(300))
            .after(1)
            .describe("observer confirms present");

        let report = runner.run().await;
        if report.status != ChainStatus::Passed {
            eprintln!("{}", report.diagnostic());
        }
        assert_eq!(report.status, ChainStatus::Passed);
    }
}
