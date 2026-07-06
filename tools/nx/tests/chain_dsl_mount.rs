// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Chain-Test: DSL in-compositor mount (TASK-0076B).
//! OWNERS: @tools-team @ui
//!
//! Verifies the mount chain hop-by-hop so a boot regression names the exact
//! failing hop instead of "windowd is silent" (the 2026-07-06 debugging that
//! motivated this file took six QEMU cycles; this pins the chain host-side):
//!
//!   windowd: present visible ok            (the mount milestone — mounts run
//!                                           here, never in the reactive frame loop)
//!   DSL: program loaded hash=…             (fail-closed validate + interpreter mount)
//!   DSL: first frame presented             (box-walker rendered into the atlas)
//!   DSL: interaction visible ok            (live tap through interpreter hit-testing)
//!
//! Failure modes are part of the contract: an under-reserved atlas pool must
//! deny with values (`need=WxH rows_remaining=N`), an invalid program must
//! fail closed — both without ever reaching the first-frame hop.

#[cfg(test)]
mod tests {
    use nx::chain::contract::{DslMountContract, DSL_ATLAS_ROWS_NEEDED};
    use nx::chain::hop::ms;
    use nx::chain::{ChainRunner, ChainStatus};

    /// Happy path: milestone → loaded → first frame → interaction, in order.
    #[tokio::test]
    async fn chain_dsl_mount_first_frame_and_interaction() {
        let mut runner = ChainRunner::new("dsl-mount");
        runner.register(Box::new(DslMountContract::healthy()));

        runner
            .expect_marker("windowd: present visible ok", ms(500))
            .describe("mount milestone: desktop composited, window pool live");
        runner
            .expect_marker("DSL: program loaded hash=0cc78eff5a933b77", ms(300))
            .after(0)
            .describe("embedded .nxir validated + interpreter mounted");
        runner
            .expect_marker("DSL: first frame presented", ms(300))
            .after(1)
            .describe("interpreter scene rendered into the atlas surface");
        runner
            .expect_marker("DSL: interaction visible ok", ms(300))
            .after(2)
            .describe("live tap dispatched through interpreter hit-testing");

        let report = runner.run().await;
        if report.status != ChainStatus::Passed {
            eprintln!("{}", report.diagnostic());
        }
        assert_eq!(report.status, ChainStatus::Passed);
    }

    /// Budget failure mode: the denial carries the exact values (the
    /// "no more guessing" rule) and the frame hop never happens.
    #[tokio::test]
    async fn chain_dsl_mount_pool_starved_denies_with_values() {
        let mut runner = ChainRunner::new("dsl-mount-pool-starved");
        runner.register(Box::new(DslMountContract::pool_starved(71)));

        runner.expect_marker("windowd: present visible ok", ms(500));
        runner.expect_marker("DSL: program loaded hash=0cc78eff5a933b77", ms(300)).after(0);
        runner
            .expect_marker("windowd: dsl open FAIL atlas (need=300x220 rows_remaining=71)", ms(300))
            .after(1)
            .describe("atlas denial names need + have — silent denial is a violation");

        let report = runner.run().await;
        if report.status != ChainStatus::Passed {
            eprintln!("{}", report.diagnostic());
        }
        assert_eq!(report.status, ChainStatus::Passed);
        assert!(
            report.hops.iter().all(|h| !h.marker.contains("first frame presented")),
            "a starved pool must never reach the first-frame hop"
        );
    }

    /// Fail-closed validation: honest FAILED marker, no window, no frame.
    #[tokio::test]
    async fn chain_dsl_mount_invalid_program_fails_closed() {
        let mut runner = ChainRunner::new("dsl-mount-invalid");
        runner.register(Box::new(DslMountContract::invalid_program()));

        runner.expect_marker("windowd: present visible ok", ms(500));
        runner
            .expect_marker("DSL: program mount FAILED (validation)", ms(300))
            .after(0)
            .describe("tampered/incompatible payload is refused before any window state");

        let report = runner.run().await;
        assert_eq!(report.status, ChainStatus::Passed);
        assert!(report
            .hops
            .iter()
            .all(|h| !h.marker.contains("program loaded") && !h.marker.contains("first frame")));
    }

    /// The reserve contract: the pool must budget the DSL window's bands.
    #[test]
    fn atlas_reserve_contract_matches_window_demand() {
        assert_eq!(DSL_ATLAS_ROWS_NEEDED, 440, "content + blur bands at 220 rows each");
    }
}
