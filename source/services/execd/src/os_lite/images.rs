// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: execd's images (ADR-0080): the embedded bytes behind each image id and the ONE
//! sealed object made of them at the first spawn and kept for every later one — every process
//! of an image maps the same frames (execd's page cache of its images). Split out of
//! `os_lite.rs` (structure gate).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: every exec'd child of the QEMU ladder; `KINIT: exec image (… mappers=2)` for
//!   the second app-host on the visible lane
//! ADR: docs/adr/0080-a-process-runs-from-a-sealed-image-text-mapped-from-its-frames-data-copied-no-other-exec-path.md

use super::{
    apphost_payload, emit_line, State, IMG_APPHOST, IMG_CACHE, IMG_EXIT0, IMG_EXIT42, IMG_FAULT,
    IMG_HELLO, IMG_VMO_HOLDER,
};
use demo_exit0::{
    DEMO_CACHE_ELF, DEMO_EXIT0_ELF, DEMO_FAULT_ELF, DEMO_MINIDUMP_ELF, DEMO_VMO_HOLDER_ELF,
};
use exec_payloads::HELLO_ELF;

/// Image ids are 1..=7: one sealed-image slot per id.
pub(super) const IMAGE_IDS: usize = 8;

/// The embedded bytes behind an image id (`None`: unknown, or app-host absent from this build).
fn image_bytes(image_id: u8) -> Option<&'static [u8]> {
    match image_id {
        IMG_HELLO => Some(HELLO_ELF),
        IMG_EXIT0 => Some(DEMO_EXIT0_ELF),
        IMG_EXIT42 => Some(DEMO_MINIDUMP_ELF),
        IMG_FAULT => Some(DEMO_FAULT_ELF),
        IMG_VMO_HOLDER => Some(DEMO_VMO_HOLDER_ELF),
        IMG_CACHE => Some(DEMO_CACHE_ELF),
        IMG_APPHOST if !apphost_payload::APPHOST_ELF.is_empty() => {
            Some(apphost_payload::APPHOST_ELF)
        }
        _ => None,
    }
}

impl State {
    /// The sealed image of `image_id`, made once (ADR-0080).
    pub(super) fn sealed_image(&mut self, image_id: u8) -> Option<u32> {
        let slot = self.images.get_mut(image_id as usize)?;
        if slot.is_none() {
            let bytes = image_bytes(image_id)?;
            match nexus_abi::sealed_image(bytes) {
                Ok(image) => *slot = Some(image),
                Err(_) => {
                    emit_line("execd: FAIL image seal");
                    return None;
                }
            }
        }
        *slot
    }
}
