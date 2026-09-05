// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host proof of `bundle-delta` (RFC-0089 §12.4 kind 4, TASK-0035
//! P3) over the REAL engine + assembler + `DeltaAdapter`: a set whose
//! changed bundle ships as an `.nxdelta` stream against the ACTIVE volume's
//! window reconstructs byte-identically to the host build; a base digest
//! absent from the active volume is `delta-base` BEFORE any write; a
//! stream whose target is not in the NEW index is `bundle-not-in-index`.
//! OWNERS: @runtime @security
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 3 tests (accept, test_reject_delta_base_bundle,
//!   test_reject_delta_target_not_in_index)
//! ADR: docs/adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md

#[path = "common/volume_scene.rs"]
mod volume_scene;
use volume_scene::*;

use updates::bundle_delta::SetSink;

/// A kind-4 component: `.nxdelta` base → target window, `kind_data` = the
/// base window digest.
fn delta_comp(base_window: &[u8], target_window: &[u8], name: &str) -> Comp {
    let stream = nxdelta::make::make(base_window, target_window);
    (
        KIND_BUNDLE_DELTA,
        name.to_string(),
        format!("bundles/{name}.nxdelta"),
        stream,
        sha256(base_window).to_vec(),
    )
}

/// The OS `StageSink` shape with the kind-4 dispatcher: kind 1 discarded,
/// kinds 6/2/4 through `SetSink` (base handle = a clone of the active fake).
struct DeltaSink {
    set: SetSink<VecDev, VecDev, Recorder, Box<dyn FnMut() -> Option<VecDev>>>,
    boot_component: bool,
}

impl updates::component_set::ComponentSink for DeltaSink {
    fn begin(&mut self, meta: &updates::component_set::ComponentMeta) -> Result<(), RejectReason> {
        self.boot_component = meta.kind == KIND_BOOT_IMAGE;
        if self.boot_component {
            return Ok(());
        }
        self.set.begin(meta)
    }
    fn chunk(&mut self, offset: u64, bytes: &[u8]) -> Result<(), RejectReason> {
        if self.boot_component {
            return Ok(());
        }
        self.set.chunk(offset, bytes)
    }
    fn finish(&mut self, meta: &updates::component_set::ComponentMeta) -> Result<(), RejectReason> {
        if self.boot_component {
            return Ok(());
        }
        self.set.finish(meta)
    }
    fn commit_set(&mut self) -> Result<(), RejectReason> {
        self.set.commit_set()
    }
}

fn run_delta(set: &[u8], active: VecDev) -> (Result<(), RejectReason>, VecDev, Vec<String>) {
    let base_active = active.clone();
    let assembler =
        VolumeAssembler::new(VecDev::new(4096), Some(active), None, Recorder::default());
    let mut sink = DeltaSink {
        set: SetSink::new(assembler, Box::new(move || Some(base_active.clone()))),
        boot_component: false,
    };
    let outcome =
        verify_and_apply(set, &Ed25519Verifier, &trust(), 0, &mut sink, &mut || {}).map(|_| ());
    let assembler = sink.set.into_assembler().expect("assembler back after the set");
    let log = assembler.events().log.clone();
    (outcome, assembler.into_inactive(), log)
}

/// The old (active) alpha window and the new one from the scene volumes.
fn windows() -> (Scene, Vec<u8>, Vec<u8>) {
    let s = scene();
    let (old_vol, old_index) = volume("1.0.0", &[0x11; 5000]);
    let old_alpha = window(&old_vol, &old_index, "alpha");
    let new_alpha = window(&s.new_vol, &s.new_index, "alpha");
    (s, old_alpha, new_alpha)
}

#[test]
fn bundle_delta_reconstructs_byte_identical_from_the_active_window() {
    let (s, old_alpha, new_alpha) = windows();
    let set = container(
        "newB",
        2,
        &[
            boot_comp(&s.k, "newB", 2),
            volume_comp(&s.new_vol, &s.new_index, &s.new_nxsv),
            delta_comp(&old_alpha, &new_alpha, "alpha@2.0.0"),
        ],
    );
    let (outcome, dev, log) = run_delta(&set, s.active);
    assert_eq!(outcome, Ok(()), "{log:?}");
    assert_eq!(dev.body(s.new_vol.len()), &s.new_vol[..]);
    assert_eq!(&dev.sectors[..512], &s.new_nxsv[..]);
    // The reconstructed window went through the assembler's verified path,
    // the untouched ones were reused.
    assert!(log.contains(&"bundle alpha@2.0.0".to_string()), "{log:?}");
    assert!(log.contains(&"reused beta@1.0.0".to_string()), "{log:?}");
    assert!(log.contains(&"reused gamma@1.0.0".to_string()), "{log:?}");
    // The stream is smaller than the window it reconstructs (a real delta).
    let stream = nxdelta::make::make(&old_alpha, &new_alpha);
    assert!(stream.len() < new_alpha.len() + 256);
}

#[test]
fn test_reject_delta_base_bundle() {
    // The base named by kind_data is NOT on the active volume: `delta-base`
    // before any window byte is written (the untouched inactive stays blank
    // past the index).
    let (s, _old_alpha, new_alpha) = windows();
    let bogus_base: Vec<u8> = (0..5000u32).map(|i| (i % 7) as u8).collect();
    let set = container(
        "newB",
        2,
        &[
            boot_comp(&s.k, "newB", 2),
            volume_comp(&s.new_vol, &s.new_index, &s.new_nxsv),
            delta_comp(&bogus_base, &new_alpha, "alpha@2.0.0"),
        ],
    );
    let (outcome, dev, _log) = run_delta(&set, s.active);
    assert_eq!(outcome, Err(RejectReason::DeltaBase));
    let alpha = s.new_index.bundle("alpha", "2.0.0").expect("row");
    let start = VOLUME_START_SECTOR as usize * 512
        + s.new_index.superblock.data_offset
        + alpha.data_offset as usize;
    assert!(dev.sectors[start..start + alpha.data_len as usize].iter().all(|&b| b == 0));
    assert!(bootfmt::nxsv::decode(&dev.sectors[..512]).is_err());
}

#[test]
fn test_reject_delta_target_not_in_index() {
    // A valid base, but the stream reconstructs a window the NEW index does
    // not carry: `bundle-not-in-index` (the assembler binds the target).
    let (s, old_alpha, _new_alpha) = windows();
    let other: Vec<u8> = (0..6000u32).map(|i| (i % 13) as u8).collect();
    let set = container(
        "newB",
        2,
        &[
            boot_comp(&s.k, "newB", 2),
            volume_comp(&s.new_vol, &s.new_index, &s.new_nxsv),
            delta_comp(&old_alpha, &other, "alpha@2.0.0"),
        ],
    );
    let (outcome, dev, _log) = run_delta(&set, s.active);
    assert_eq!(outcome, Err(RejectReason::BundleNotInIndex));
    assert!(bootfmt::nxsv::decode(&dev.sectors[..512]).is_err());
}
