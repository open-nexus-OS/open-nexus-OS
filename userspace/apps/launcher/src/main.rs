// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Minimal TASK-0055 launcher client.
//! OWNERS: @ui
//! STATUS: Done
//! TEST_COVERAGE: `cargo test -p launcher -- --nocapture`.

#![forbid(unsafe_code)]

fn main() {
    match launcher::draw_first_frame().and_then(|ack| launcher::first_frame_marker(Some(ack))) {
        Ok(marker) => {
            println!("{marker}");
        }
        Err(err) => {
            eprintln!("launcher: first frame failed: {err:?}");
            std::process::exit(1);
        }
    }
}
