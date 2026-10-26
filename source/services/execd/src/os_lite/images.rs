// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: execd's images (ADR-0080): the ONE sealed object behind each image id, kept for
//! every spawn — every process of an image maps the same frames (execd's page cache of its
//! images). The tiny probe ELFs are embedded and sealed at their first spawn; the app-host and
//! the recv-wake probe are files of the bootfs (ADR-0081) whose sealed objects init handed into
//! execd's declared slots at its spawn — never embedded, never copied. Split out of
//! `os_lite.rs` (structure gate).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: every exec'd child of the QEMU ladder; `KINIT: exec image (… mappers=2)` for
//!   the second app-host on the visible lane
//! ADR: docs/adr/0081-the-boot-image-is-one-verified-container-kernel-plus-bootfs-every-core-program-runs-from-it-no-program-embeds-another.md

use super::{
    emit_line, topo, State, IMG_APPHOST, IMG_CACHE, IMG_EXIT0, IMG_EXIT42, IMG_FAULT, IMG_HELLO,
    IMG_VMO_HOLDER,
};
use demo_exit0::{
    DEMO_CACHE_ELF, DEMO_EXIT0_ELF, DEMO_FAULT_ELF, DEMO_MINIDUMP_ELF, DEMO_VMO_HOLDER_ELF,
};
use exec_payloads::HELLO_ELF;

/// Image ids are 1..=7: one sealed-image slot per id.
pub(super) const IMAGE_IDS: usize = 8;

/// The embedded bytes behind a probe image id (`None`: not an embedded image).
fn image_bytes(image_id: u8) -> Option<&'static [u8]> {
    match image_id {
        IMG_HELLO => Some(HELLO_ELF),
        IMG_EXIT0 => Some(DEMO_EXIT0_ELF),
        IMG_EXIT42 => Some(DEMO_MINIDUMP_ELF),
        IMG_FAULT => Some(DEMO_FAULT_ELF),
        IMG_VMO_HOLDER => Some(DEMO_VMO_HOLDER_ELF),
        IMG_CACHE => Some(DEMO_CACHE_ELF),
        _ => None,
    }
}

/// A sealed image init transferred into `slot` at execd's spawn (ADR-0081). `None` = the slot is
/// empty — the image answers UNSUPPORTED then, never a fake payload.
pub(super) fn handed_over(slot: u32) -> Option<u32> {
    let mut info = nexus_abi::CapQuery::default();
    (nexus_abi::cap_query(slot, &mut info).is_ok() && info.kind_tag == 3).then_some(slot)
}

impl State {
    /// The sealed image of `image_id`: the handed-over object for the app-host, the embedded
    /// probe sealed once (ADR-0080) for the rest.
    pub(super) fn sealed_image(&mut self, image_id: u8) -> Option<u32> {
        let slot = self.images.get_mut(image_id as usize)?;
        if slot.is_none() {
            *slot = if image_id == IMG_APPHOST {
                handed_over(topo::APPHOST_IMAGE)
            } else {
                match nexus_abi::sealed_image(image_bytes(image_id)?) {
                    Ok(image) => Some(image),
                    Err(_) => {
                        emit_line("execd: FAIL image seal");
                        return None;
                    }
                }
            };
        }
        *slot
    }
}
