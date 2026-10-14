// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Control-stream words (TASK-0215 5b, RFC-0101 §8d): the handful of geometry-stream (VDM),
//! primitive-pipeline (PPP) and PDS words the device writes itself for a draw, packed as the
//! open driver's word-layout data files describe them (`vdm.xml`, `ppp.xml`, `pds.xml` of its
//! control-stream generator; the capture of 2026-10-14 is the golden for every word here).
//! Addresses in the PDS heap are heap offsets (the heap's base is implicit); PPP block
//! addresses are full 40-bit addresses split into an MSB byte and a word-aligned LSB.

/// The geometry stream (VDM).
pub mod vdm {
    /// Block types, bits 29–31 of a record's first word.
    pub const BLOCK_PPP_STATE: u32 = 0;
    pub const BLOCK_PDS_STATE: u32 = 1;
    pub const BLOCK_VDM_STATE: u32 = 2;
    pub const BLOCK_INDEX_LIST: u32 = 3;
    pub const BLOCK_TERMINATE: u32 = 6;

    /// `PPP_STATE_UPDATE`: `word_count` state words at `addr`.
    pub const fn ppp_state(word_count: u32, addr: u64) -> [u32; 2] {
        [
            (BLOCK_PPP_STATE << 29) | ((word_count & 0xff) << 8) | (((addr >> 32) & 0xff) as u32),
            (addr as u32) & !3,
        ]
    }

    /// `VDM_STATE_UPDATE` with nothing present: the camera size alone (the open driver's
    /// record before every draw but the first).
    pub const fn vdm_state_bare(cam_size: u32) -> u32 {
        (BLOCK_VDM_STATE << 29) | (((cam_size - 1) & 0xff) << 7)
    }

    /// `INDEX_LIST`: `count` indices of `topology` (6 = triangle list), 8-bit index size, no
    /// index buffer, with an optional index offset.
    pub fn index_list(count: u32, offset: Option<u32>, topology: u32) -> ([u32; 3], usize) {
        let mut w0 = (BLOCK_INDEX_LIST << 29) | (1 << 27) | ((topology & 0xf) << 8);
        let mut words = [w0, count, 0];
        let n = match offset {
            Some(off) => {
                w0 |= 1 << 25;
                words[0] = w0;
                words[2] = off;
                3
            }
            None => 2,
        };
        (words, n)
    }

    /// `TERMINATE` of the stream.
    pub const TERMINATE: u32 = BLOCK_TERMINATE << 29;
}

/// The primitive-pipeline state blocks (PPP).
pub mod ppp {
    /// `STATE_HEADER` presence bits.
    pub const PRES_PDS_STATE_PTR0: u32 = 1 << 6;
    pub const PRES_PDS_STATE_PTR1: u32 = 1 << 7;
    pub const PRES_PDS_STATE_PTR2: u32 = 1 << 8;
    pub const PRES_PDS_STATE_PTR3: u32 = 1 << 9;
    /// A block that updates the fragment PDS pointers only (the open driver's per-draw block).
    pub const HEADER_PDS_ONLY: u32 =
        PRES_PDS_STATE_PTR0 | PRES_PDS_STATE_PTR1 | PRES_PDS_STATE_PTR3;

    /// The fragment PDS state: programs and sizes.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct PdsState {
        /// PDS heap offsets (16-byte aligned).
        pub shaderbase: u32,
        pub texunicodebase: u32,
        pub varyingbase: u32,
        pub uniformdatabase: u32,
        /// `STATE_PDS_SIZEINFO1`: uniform, texture-state, varying (PDS), varying (USC), temp.
        pub sizeinfo1: u32,
        /// `STATE_PDS_SIZEINFO2`: shared size, tri-merge disable, batch number.
        pub sizeinfo2: u32,
    }

    impl PdsState {
        /// The six words after a `HEADER_PDS_ONLY` header, in the open driver's order.
        pub const fn words(&self) -> [u32; 6] {
            [
                self.shaderbase & !0xf,
                self.texunicodebase & !0xf,
                self.sizeinfo1,
                self.sizeinfo2,
                self.varyingbase & !0xf,
                self.uniformdatabase & !0xf,
            ]
        }

        /// A seven-word per-draw block: header and the six words.
        pub const fn block(&self) -> [u32; 7] {
            let w = self.words();
            [HEADER_PDS_ONLY, w[0], w[1], w[2], w[3], w[4], w[5]]
        }
    }

    /// `STATE_PDS_SIZEINFO1`.
    pub const fn sizeinfo1(
        pds_uniform: u32,
        pds_texture_state: u32,
        pds_varying: u32,
        usc_varying: u32,
        pds_temp: u32,
    ) -> u32 {
        ((pds_uniform & 0x1ff) << 23)
            | ((pds_texture_state & 0x7f) << 16)
            | ((pds_varying & 0x3f) << 10)
            | ((usc_varying & 0x3f) << 4)
            | (pds_temp & 0xf)
    }

    /// `STATE_PDS_SIZEINFO2`.
    pub const fn sizeinfo2(usc_shared: u32, tri_merge_disable: bool, batch_num: u32) -> u32 {
        ((usc_shared & 0x1ff) << 23) | ((tri_merge_disable as u32) << 14) | (batch_num & 0x3fff)
    }
}

/// PDS instruction fields.
pub mod pds {
    /// Rewrites the 40-bit `sbase` of a `DOUTD` source-0 pair in place, keeping `doffset` and
    /// the cache mode (bits 40–61).
    pub fn set_doutd_sbase(pair: &mut [u8], sbase: u64) {
        let mut w = [0u8; 8];
        w.copy_from_slice(&pair[..8]);
        let old = u64::from_le_bytes(w);
        let new = (old & !((1u64 << 40) - 1)) | (sbase & ((1u64 << 40) - 1));
        pair[..8].copy_from_slice(&new.to_le_bytes());
    }

    /// The 40-bit `sbase` of a `DOUTD` source-0 pair.
    pub fn doutd_sbase(pair: &[u8]) -> u64 {
        let mut w = [0u8; 8];
        w.copy_from_slice(&pair[..8]);
        u64::from_le_bytes(w) & ((1u64 << 40) - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_captured_stream_words_pack() {
        // 2026-10-14 capture: `PPP_STATE_UPDATE` 26 words at 0x8000053200 → 80 1a 00 00 | 00 32 05 00.
        assert_eq!(vdm::ppp_state(26, 0x80_0005_3200), [0x0000_1a80, 0x0005_3200]);
        // The two-quad capture's second-draw block: 7 words at 0x8000057340.
        assert_eq!(vdm::ppp_state(7, 0x80_0005_7340), [0x0000_0780, 0x0005_7340]);
        // The bare VDM state before the second draw: 80 0f 00 40.
        assert_eq!(vdm::vdm_state_bare(32), 0x4000_0f80);
        // The first draw's index list (6, no offset) and the second's (6, offset 6).
        assert_eq!(vdm::index_list(6, None, 6), ([0x6800_0600, 6, 0], 2));
        assert_eq!(vdm::index_list(6, Some(6), 6), ([0x6a00_0600, 6, 6], 3));
        assert_eq!(vdm::TERMINATE, 0xc000_0000);
    }

    #[test]
    fn the_captured_pds_state_packs() {
        let s = ppp::PdsState {
            shaderbase: 0x2900,
            texunicodebase: 0x2700,
            varyingbase: 0x2940,
            uniformdatabase: 0x2880,
            sizeinfo1: ppp::sizeinfo1(3, 0, 1, 1, 1),
            sizeinfo2: ppp::sizeinfo2(2, true, 0),
        };
        assert_eq!(s.sizeinfo1, 0x0180_0411);
        assert_eq!(s.sizeinfo2, 0x0100_4000);
        assert_eq!(s.words(), [0x2900, 0x2700, 0x0180_0411, 0x0100_4000, 0x2940, 0x2880]);
        assert_eq!(ppp::HEADER_PDS_ONLY, 0x2c0);
        assert_eq!(s.block()[0], 0x2c0);
    }

    #[test]
    fn doutd_sbase_is_rewritten_in_place() {
        // The first draw's push-constant descriptor: sbase 0x800004b100, doffset/cmode above.
        let mut pair = [0x00, 0xb1, 0x04, 0x00, 0x80, 0x00, 0x00, 0x00];
        assert_eq!(pds::doutd_sbase(&pair), 0x80_0004_b100);
        pds::set_doutd_sbase(&mut pair, 0x80_0005_3280);
        assert_eq!(pair, [0x80, 0x32, 0x05, 0x00, 0x80, 0x00, 0x00, 0x00]);
        let mut with_high = [0x00, 0xb1, 0x04, 0x00, 0x80, 0x12, 0x00, 0x10];
        pds::set_doutd_sbase(&mut with_high, 0x80_0005_3280);
        assert_eq!(&with_high[5..], &[0x12, 0x00, 0x10], "bits above 40 kept");
    }
}
