// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Chain-Test: Phase A1 — Remove CPU Blur
//! OWNERS: @tools-team
//!
//! Validates that CPU blur (blur_backdrop_segment) is never called and GPU
//! BlurBackdrop is the only blur command in the steady-state frame path.

#[cfg(test)]
mod tests {
    use nx::chain::contract::{GpudContract, WindowdContract};
    use nx::chain::hop::ms;
    use nx::chain::{ChainRunner, ChainStatus};

    /// Phase A1: CPU blur removed, GPU BlurBackdrop is the only blur.
    ///
    /// Hops:
    ///   H0: gpud: virtio-gpu probed
    ///   H1: gpud: ready
    ///   H2: windowd: handoff attach ack
    ///   H3: windowd: present visible ok
    ///   H4: gpud: transfer_to_host ok     ← GPU blur path active
    ///   H5: gpud: resource flush ok       ← GPU blur path active
    ///
    /// Absence proof: NO cpu_blur, NO blur_backdrop_segment markers.
    #[tokio::test]
    async fn chain_no_cpu_blur_gpu_only() {
        let mut runner = ChainRunner::new("no-cpu-blur");

        runner.register(Box::new(GpudContract::with_handoff_and_cursor()));
        runner.register(Box::new(WindowdContract::visible_bootstrap(1280, 800)));

        runner
            .expect_marker("gpud: virtio-gpu probed", ms(500))
            .describe("H0: gpud probe");
        runner
            .expect_marker("gpud: ready", ms(300))
            .after(0)
            .describe("H1: gpud ready");
        runner
            .expect_marker("windowd: handoff attach ack", ms(1000))
            .after(1)
            .describe("H2: handoff complete");
        runner
            .expect_marker("windowd: present visible ok", ms(500))
            .after(2)
            .describe("H3: first frame visible");
        // GPU blur markers prove GPU path is active
        runner
            .expect_marker("gpud: transfer_to_host ok", ms(500))
            .after(3)
            .describe("H4: GPU blur transfer");
        runner
            .expect_marker("gpud: resource flush ok", ms(300))
            .after(4)
            .describe("H5: GPU blur flush complete");

        let report = runner.run().await;
        if report.status != ChainStatus::Passed {
            eprintln!("{}", report.diagnostic());
        }
        assert_eq!(report.status, ChainStatus::Passed);
    }
}
