// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Chain-Test: input pipeline (device → hidrawd → inputd → windowd)
//! OWNERS: @tools-team
//!
//! Verifies the input-chain hop order I1..I6. Mirrors the real GPUD/inputd/
//! hidrawd/windowd `chain I#` markers so a headless OS run shows exactly how far
//! an input event gets; this spec pins the order. The present chain (gpud
//! G1..G4, chain_gpu_scanout.rs) takes over from I6.

#[cfg(test)]
mod tests {
    use nx::chain::contract::{HidrawdContract, InputdContract, WindowdContract};
    use nx::chain::hop::ms;
    use nx::chain::{ChainRunner, ChainStatus};

    /// Chain: an input event travels device → hidrawd → inputd → windowd.
    #[tokio::test]
    async fn chain_input_hops_in_order() {
        let mut runner = ChainRunner::new("input-hops");

        runner.register(Box::new(HidrawdContract::with_input()));
        runner.register(Box::new(InputdContract::with_cursor_moves()));
        runner.register(Box::new(WindowdContract::visible_bootstrap(1280, 800)));

        runner
            .expect_marker("hidrawd: chain I1 device event (raw HID polled)", ms(1000))
            .describe("I1: raw HID event polled from a virtio-input device");
        runner
            .expect_marker("hidrawd: chain I2 wire sent to inputd", ms(300))
            .after(0)
            .describe("I2: normalized wire batch sent to inputd");
        runner
            .expect_marker("inputd: chain I3 wire recv from hidrawd", ms(300))
            .after(1)
            .describe("I3: inputd received the wire batch");
        runner
            .expect_marker("inputd: chain I4 normalized", ms(200))
            .after(2)
            .describe("I4: wire batch decoded into normalized events");
        runner
            .expect_marker("inputd: chain I5 delivered to windowd", ms(300))
            .after(3)
            .describe("I5: visible-state delivered to windowd");
        runner
            .expect_marker("windowd: chain I6 input recv (state applied)", ms(500))
            .after(4)
            .describe("I6: windowd applied the input (handoff to the present chain)");

        let report = runner.run().await;
        if report.status != ChainStatus::Passed {
            eprintln!("{}", report.diagnostic());
        }
        assert_eq!(report.status, ChainStatus::Passed);
    }
}
