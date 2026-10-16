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
    /// `STATE_HEADER` presence bits: the ISP words (control, front A — its pass type —, front
    /// B), the PDS state pointers.
    pub const PRES_ISPCTL: u32 = 1 << 0;
    pub const PRES_ISPCTL_FA: u32 = 1 << 1;
    pub const PRES_ISPCTL_FB: u32 = 1 << 2;
    pub const PRES_PDS_STATE_PTR0: u32 = 1 << 6;
    pub const PRES_PDS_STATE_PTR1: u32 = 1 << 7;
    pub const PRES_PDS_STATE_PTR2: u32 = 1 << 8;
    pub const PRES_PDS_STATE_PTR3: u32 = 1 << 9;
    /// A block that updates the fragment PDS pointers only (the open driver's per-draw block).
    pub const HEADER_PDS_ONLY: u32 =
        PRES_PDS_STATE_PTR0 | PRES_PDS_STATE_PTR1 | PRES_PDS_STATE_PTR3;
    /// A block that re-states the ISP words with the PDS pointers: a draw whose pass type
    /// differs from the draw before (the words in the presence bits' order).
    pub const HEADER_ISP_PDS: u32 = PRES_ISPCTL | PRES_ISPCTL_FA | PRES_ISPCTL_FB | HEADER_PDS_ONLY;

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

        /// A ten-word per-draw block: header, the ISP words `isp` (control, front A, front B),
        /// the six words.
        pub const fn block_with_isp(&self, isp: [u32; 3]) -> [u32; 10] {
            let w = self.words();
            [HEADER_ISP_PDS, isp[0], isp[1], isp[2], w[0], w[1], w[2], w[3], w[4], w[5]]
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

    /// `STATE_ISPA` with its pass type (bits 24–26): opaque 0, translucent 1, punch-through 2.
    pub const fn with_pass_type(ispa: u32, pass_type: u8) -> u32 {
        (ispa & !(7 << 24)) | (((pass_type & 7) as u32) << 24)
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

/// The pixel back end's register words (`pbestate.xml`): the render-area clip, pixel-precise on
/// all four edges — what the open driver changes for a smaller render area (2026-10-14).
pub mod pbe {
    const CLIP: u64 = 0x3fff;

    /// `REG_WORD0` with `minclip_x` (bits 6–19) replaced.
    pub const fn with_min_x(word0: u64, min_x: u32) -> u64 {
        (word0 & !(CLIP << 6)) | (((min_x as u64) & CLIP) << 6)
    }

    /// `REG_WORD1` with `minclip_y` (46–59), `maxclip_x` (32–45), `maxclip_y` (0–13) replaced.
    pub const fn with_clip(word1: u64, min_y: u32, max_x: u32, max_y: u32) -> u64 {
        (word1 & !((CLIP << 46) | (CLIP << 32) | CLIP))
            | (((min_y as u64) & CLIP) << 46)
            | (((max_x as u64) & CLIP) << 32)
            | ((max_y as u64) & CLIP)
    }

    /// The clip `(min_x, min_y, max_x, max_y)` the two words carry.
    pub const fn clip(word0: u64, word1: u64) -> (u32, u32, u32, u32) {
        (
            ((word0 >> 6) & CLIP) as u32,
            ((word1 >> 46) & CLIP) as u32,
            ((word1 >> 32) & CLIP) as u32,
            (word1 & CLIP) as u32,
        )
    }
}

/// The fragment job's `ISP_CTL` bits the device changes.
pub mod isp {
    /// Run the background object on tiles without geometry. The open driver clears it for a
    /// render area smaller than the target, clearing by geometry instead: tiles outside the area
    /// then cost nothing (2026-10-14).
    pub const CTL_PROCESS_EMPTY_TILES: u32 = 1 << 17;
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
        // The captured 26-word block's header (0x007f0ee7) presents the three ISP words and the
        // PDS pointers the per-draw blocks re-state.
        assert_eq!(0x007f_0ee7 & ppp::HEADER_ISP_PDS, ppp::HEADER_ISP_PDS);
        assert_eq!(ppp::HEADER_ISP_PDS, 0x2c7);
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
    fn the_render_area_clip_is_the_captured_one() {
        // c_full and c_area of 2026-10-14: the full plane, then 704..1215 x 284..795.
        let (full0, full1) = (0x0200_0efc_60a0_0000u64, 0x0000_077f_0000_0437u64);
        let (area0, area1) = (0x0200_0efc_60a0_b000u64, 0x0047_04bf_0000_031bu64);
        assert_eq!(pbe::clip(full0, full1), (0, 0, 1919, 1079));
        assert_eq!(pbe::with_min_x(full0, 704), area0);
        assert_eq!(pbe::with_clip(full1, 284, 1215, 795), area1);
        assert_eq!(pbe::clip(area0, area1), (704, 284, 1215, 795));
        // The captured ISP control words: process_empty_tiles on for the full plane, off for the area.
        assert_eq!(0x8003_1000u32 & !isp::CTL_PROCESS_EMPTY_TILES, 0x8001_1000);
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
