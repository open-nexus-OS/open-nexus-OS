// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: A bounded EDID 1.4 + CEA-861 parser and the ONE mode policy (TASK-0250 P1). The
//! monitor's bytes are untrusted input: the base block and every extension are length- and
//! checksum-checked before a field is read, every descriptor read is bounds-checked, the mode
//! list is a fixed array, and nothing here can panic on any input (the `test_reject_*` tests
//! below feed truncated, corrupted and overflowing blocks). Goldens: the reference board's
//! monitor (`docs/board/measurements/2026-09-29-display-regs/edid-hdmi-2026-09-29.bin`, decoded
//! with `edid-decode` in `edid-decode.txt`): preferred 2560x1440, VIC 16 and a detailed
//! 1920x1080@60 timing (h 88/44/148, v 2/5/38 — the stock system's mode line), 16:9.
//!
//! `pick_mode`: the highest progressive mode whose size fits the SoC's maximum and whose aspect
//! is the monitor's own (no stretching — the user's rule 2026-09-29), 60 Hz preferred; a
//! detailed timing beats the standard table for the same size (it is what the monitor asks
//! for, and what the stock system drove).
//! OWNERS: @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below + `tests/dc_goldens.rs`

/// One progressive video mode with its full timing, as the display controller needs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mode {
    pub h_active: u16,
    pub v_active: u16,
    pub hfp: u16,
    pub hsw: u16,
    pub hbp: u16,
    pub vfp: u16,
    pub vsw: u16,
    pub vbp: u16,
    pub hsync_positive: bool,
    pub vsync_positive: bool,
    /// Pixel clock in kHz (148_500 for 1080p60).
    pub pixel_clock_khz: u32,
    /// Whether the monitor gave this timing itself (a detailed timing descriptor) rather
    /// than by a standard-table identifier.
    pub detailed: bool,
}

impl Mode {
    /// Total pixels per line and lines per frame.
    pub const fn h_total(&self) -> u32 {
        self.h_active as u32 + self.hfp as u32 + self.hsw as u32 + self.hbp as u32
    }
    pub const fn v_total(&self) -> u32 {
        self.v_active as u32 + self.vfp as u32 + self.vsw as u32 + self.vbp as u32
    }
    /// Refresh rate in millihertz (59_940 for the 1080p60 timing above).
    pub fn refresh_mhz(&self) -> u32 {
        let total = self.h_total() as u64 * self.v_total() as u64;
        if total == 0 {
            return 0;
        }
        ((self.pixel_clock_khz as u64 * 1_000_000) / total) as u32
    }
    pub const fn area(&self) -> u32 {
        self.h_active as u32 * self.v_active as u32
    }
}

/// The modes an EDID names, at most [`ModeList::CAP`] (four base descriptors, a CEA block's
/// short video descriptors and detailed timings — 24 covers every monitor we can drive).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeList {
    modes: [Mode; ModeList::CAP],
    len: usize,
}

impl ModeList {
    pub const CAP: usize = 24;
    const EMPTY: Mode = Mode {
        h_active: 0,
        v_active: 0,
        hfp: 0,
        hsw: 0,
        hbp: 0,
        vfp: 0,
        vsw: 0,
        vbp: 0,
        hsync_positive: false,
        vsync_positive: false,
        pixel_clock_khz: 0,
        detailed: false,
    };
    pub const fn new() -> Self {
        Self { modes: [Self::EMPTY; Self::CAP], len: 0 }
    }
    /// Appends `mode`; a full list drops the mode (bounded on purpose, never an error the
    /// monitor could provoke into a panic).
    pub fn push(&mut self, mode: Mode) {
        if self.len < Self::CAP {
            self.modes[self.len] = mode;
            self.len += 1;
        }
    }
    pub fn as_slice(&self) -> &[Mode] {
        &self.modes[..self.len]
    }
    pub const fn len(&self) -> usize {
        self.len
    }
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for ModeList {
    fn default() -> Self {
        Self::new()
    }
}

/// What the parser refuses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdidError {
    /// Fewer than 128 bytes, or fewer than the extension count announces.
    Truncated,
    /// The eight-byte header is not the EDID magic.
    BadHeader,
    /// A block's bytes do not sum to zero modulo 256 (`block` = its index).
    BadChecksum { block: u8 },
    /// More extension blocks than this parser bounds ([`Edid::MAX_EXTENSIONS`]).
    TooManyExtensions,
}

/// The parsed result: the modes and what the monitor said about itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edid {
    pub modes: ModeList,
    /// The first detailed timing of the base block (the monitor's preferred mode), if any.
    pub preferred: Option<Mode>,
    /// Screen size in centimetres (width, height) when the monitor names it.
    pub screen_cm: Option<(u8, u8)>,
}

impl Edid {
    /// Extension blocks accepted beyond the base block (one CEA block is the common case).
    pub const MAX_EXTENSIONS: usize = 2;
}

const BLOCK: usize = 128;
const HEADER: [u8; 8] = [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00];

/// Parses `bytes` (the base block plus its extensions). Bounded: at most
/// `128 * (1 + MAX_EXTENSIONS)` bytes are looked at, every block is checksummed first.
pub fn parse_edid(bytes: &[u8]) -> Result<Edid, EdidError> {
    if bytes.len() < BLOCK {
        return Err(EdidError::Truncated);
    }
    let base = &bytes[..BLOCK];
    if base[..8] != HEADER {
        return Err(EdidError::BadHeader);
    }
    if !checksum_ok(base) {
        return Err(EdidError::BadChecksum { block: 0 });
    }
    let extensions = base[126] as usize;
    if extensions > Edid::MAX_EXTENSIONS {
        return Err(EdidError::TooManyExtensions);
    }
    if bytes.len() < BLOCK * (1 + extensions) {
        return Err(EdidError::Truncated);
    }
    let mut modes = ModeList::new();
    let mut preferred = None;
    // Four 18-byte descriptors at 0x36; a descriptor whose pixel clock is non-zero is a
    // detailed timing (the first is the preferred mode by EDID 1.4's rule).
    for i in 0..4 {
        let off = 0x36 + i * 18;
        if let Some(mode) = parse_dtd(&base[off..off + 18]) {
            if preferred.is_none() {
                preferred = Some(mode);
            }
            modes.push(mode);
        }
    }
    let screen_cm = match (base[0x15], base[0x16]) {
        (0, _) | (_, 0) => None,
        (w, h) => Some((w, h)),
    };
    for e in 0..extensions {
        let block = &bytes[BLOCK * (1 + e)..BLOCK * (2 + e)];
        if !checksum_ok(block) {
            return Err(EdidError::BadChecksum { block: (1 + e) as u8 });
        }
        if block[0] == 0x02 {
            parse_cea(block, &mut modes);
        }
    }
    Ok(Edid { modes, preferred, screen_cm })
}

fn checksum_ok(block: &[u8]) -> bool {
    block.iter().fold(0u8, |acc, b| acc.wrapping_add(*b)) == 0
}

/// A detailed timing descriptor (18 bytes) → a progressive mode; `None` for a monitor
/// descriptor (pixel clock 0), an interlaced timing or a degenerate size.
fn parse_dtd(d: &[u8]) -> Option<Mode> {
    if d.len() < 18 {
        return None;
    }
    let pixel_clock_khz = u16::from_le_bytes([d[0], d[1]]) as u32 * 10;
    if pixel_clock_khz == 0 {
        return None;
    }
    let h_active = d[2] as u16 | (((d[4] >> 4) as u16) << 8);
    let h_blank = d[3] as u16 | (((d[4] & 0x0f) as u16) << 8);
    let v_active = d[5] as u16 | (((d[7] >> 4) as u16) << 8);
    let v_blank = d[6] as u16 | (((d[7] & 0x0f) as u16) << 8);
    let hfp = d[8] as u16 | (((d[11] >> 6) as u16) << 8);
    let hsw = d[9] as u16 | ((((d[11] >> 4) & 0x03) as u16) << 8);
    let vfp = (d[10] >> 4) as u16 | ((((d[11] >> 2) & 0x03) as u16) << 4);
    let vsw = (d[10] & 0x0f) as u16 | (((d[11] & 0x03) as u16) << 4);
    let flags = d[17];
    if flags & 0x80 != 0 || h_active == 0 || v_active == 0 {
        return None; // interlaced, or nothing to show
    }
    let hbp = h_blank.checked_sub(hfp)?.checked_sub(hsw)?;
    let vbp = v_blank.checked_sub(vfp)?.checked_sub(vsw)?;
    Some(Mode {
        h_active,
        v_active,
        hfp,
        hsw,
        hbp,
        vfp,
        vsw,
        vbp,
        hsync_positive: flags & 0x02 != 0,
        vsync_positive: flags & 0x04 != 0,
        pixel_clock_khz,
        detailed: true,
    })
}

/// A CEA-861 extension block: the video data block's short video descriptors (VICs we know
/// the timing of) and the detailed timings after the data block collection.
fn parse_cea(block: &[u8], modes: &mut ModeList) {
    let dtd_off = block[2] as usize;
    if block[1] >= 3 && dtd_off >= 4 {
        let mut i = 4usize;
        while i < dtd_off.min(BLOCK) {
            let tag = block[i] >> 5;
            let len = (block[i] & 0x1f) as usize;
            let end = (i + 1 + len).min(dtd_off.min(BLOCK));
            if tag == 2 {
                for &svd in &block[i + 1..end] {
                    if let Some(mode) = vic_mode(svd & 0x7f) {
                        modes.push(mode);
                    }
                }
            }
            i = i + 1 + len;
        }
    }
    if dtd_off >= 4 {
        let mut off = dtd_off;
        while off + 18 <= BLOCK - 1 {
            if let Some(mode) = parse_dtd(&block[off..off + 18]) {
                modes.push(mode);
            }
            off += 18;
        }
    }
}

/// The CEA-861 progressive timings this driver knows (interlaced identifiers are skipped —
/// the controller scans progressive frames).
fn vic_mode(vic: u8) -> Option<Mode> {
    let t = |h, v, hfp, hsw, hbp, vfp, vsw, vbp, hp, vp, clk| Mode {
        h_active: h,
        v_active: v,
        hfp,
        hsw,
        hbp,
        vfp,
        vsw,
        vbp,
        hsync_positive: hp,
        vsync_positive: vp,
        pixel_clock_khz: clk,
        detailed: false,
    };
    Some(match vic {
        1 => t(640, 480, 16, 96, 48, 10, 2, 33, false, false, 25_175),
        2 | 3 => t(720, 480, 16, 62, 60, 9, 6, 30, false, false, 27_000),
        4 => t(1280, 720, 110, 40, 220, 5, 5, 20, true, true, 74_250),
        16 => t(1920, 1080, 88, 44, 148, 4, 5, 36, true, true, 148_500),
        17 | 18 => t(720, 576, 12, 64, 68, 5, 5, 39, false, false, 27_000),
        19 => t(1280, 720, 440, 40, 220, 5, 5, 20, true, true, 74_250),
        31 => t(1920, 1080, 528, 44, 148, 4, 5, 36, true, true, 148_500),
        32 => t(1920, 1080, 638, 44, 148, 4, 5, 36, true, true, 74_250),
        33 => t(1920, 1080, 528, 44, 148, 4, 5, 36, true, true, 74_250),
        34 => t(1920, 1080, 88, 44, 148, 4, 5, 36, true, true, 74_250),
        _ => return None,
    })
}

/// The ONE mode policy: among `modes`, the progressive mode that fits `soc_max` (w, h) with the
/// monitor's aspect (`screen_cm`, when known, within 2 %), the largest area first, then the
/// refresh nearest 60 Hz, then a detailed timing over a standard one. `None` when nothing
/// fits — the caller falls back to its requested mode, never stretches.
pub fn pick_mode(modes: &[Mode], soc_max: (u32, u32), screen_cm: Option<(u8, u8)>) -> Option<Mode> {
    let fits = |m: &Mode| m.h_active as u32 <= soc_max.0 && m.v_active as u32 <= soc_max.1;
    let aspect_ok = |m: &Mode| match screen_cm {
        Some((w, h)) => {
            // |m.w * h − m.h * w| / (m.h * w) ≤ 2 %
            let lhs = m.h_active as u64 * h as u64;
            let rhs = m.v_active as u64 * w as u64;
            let diff = lhs.abs_diff(rhs);
            diff * 50 <= rhs
        }
        None => true,
    };
    let key = |m: &Mode| {
        let off60 = m.refresh_mhz().abs_diff(60_000);
        (m.area(), u32::MAX - off60, m.detailed as u8)
    };
    let mut best: Option<Mode> = None;
    for m in modes.iter().filter(|m| fits(m) && aspect_ok(m)) {
        match best {
            Some(b) if key(&b) >= key(m) => {}
            _ => best = Some(*m),
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reference board's monitor (docs/board/measurements/2026-09-29-display-regs).
    const EDID: &[u8] = include_bytes!(
        "../../../../../docs/board/measurements/2026-09-29-display-regs/edid-hdmi-2026-09-29.bin"
    );

    fn mode_1080p60_dtd() -> Mode {
        parse_edid(EDID)
            .expect("edid")
            .modes
            .as_slice()
            .iter()
            .copied()
            .find(|m| m.h_active == 1920 && m.v_active == 1080 && m.detailed)
            .expect("the monitor names 1080p60 in a detailed timing")
    }

    #[test]
    fn the_reference_monitor_parses_to_its_decoded_facts() {
        let e = parse_edid(EDID).expect("edid");
        let p = e.preferred.expect("preferred");
        assert_eq!((p.h_active, p.v_active), (2560, 1440));
        assert_eq!(p.pixel_clock_khz, 241_500);
        assert_eq!(e.screen_cm, Some((53, 30)));
        let m = mode_1080p60_dtd();
        assert_eq!((m.hfp, m.hsw, m.hbp), (88, 44, 148));
        assert_eq!((m.vfp, m.vsw, m.vbp), (2, 5, 38));
        assert_eq!((m.h_total(), m.v_total()), (2200, 1125));
        assert_eq!(m.pixel_clock_khz, 148_500);
        assert_eq!(m.refresh_mhz(), 60_000);
        assert!(m.hsync_positive && m.vsync_positive);
        // VIC 16 from the CEA block as well.
        assert!(e.modes.as_slice().iter().any(|m| m.h_active == 1920 && !m.detailed));
        assert!(e.modes.len() >= 8);
    }

    #[test]
    fn pick_mode_is_the_socs_maximum_with_the_monitors_aspect_and_timing() {
        let e = parse_edid(EDID).expect("edid");
        let m = pick_mode(e.modes.as_slice(), (1920, 1080), e.screen_cm).expect("a mode");
        assert_eq!(m, mode_1080p60_dtd());
        // A larger SoC maximum would pick the preferred 2560x1440.
        let big = pick_mode(e.modes.as_slice(), (2560, 1440), e.screen_cm).expect("a mode");
        assert_eq!((big.h_active, big.v_active), (2560, 1440));
        // A smaller one picks 720p60 (16:9), never a stretched 4:3 mode.
        let small = pick_mode(e.modes.as_slice(), (1280, 800), e.screen_cm).expect("a mode");
        assert_eq!((small.h_active, small.v_active), (1280, 720));
    }

    #[test]
    fn test_reject_bad_checksum() {
        let mut bytes = [0u8; 256];
        bytes.copy_from_slice(EDID);
        bytes[20] ^= 0x01;
        assert_eq!(parse_edid(&bytes), Err(EdidError::BadChecksum { block: 0 }));
        let mut bytes = [0u8; 256];
        bytes.copy_from_slice(EDID);
        bytes[200] ^= 0x01;
        assert_eq!(parse_edid(&bytes), Err(EdidError::BadChecksum { block: 1 }));
    }

    #[test]
    fn test_reject_truncated_and_bad_header() {
        assert_eq!(parse_edid(&EDID[..127]), Err(EdidError::Truncated));
        // Announces one extension, delivers none.
        assert_eq!(parse_edid(&EDID[..128]), Err(EdidError::Truncated));
        let mut bytes = [0u8; 256];
        bytes.copy_from_slice(EDID);
        bytes[0] = 0x01;
        assert_eq!(parse_edid(&bytes), Err(EdidError::BadHeader));
        assert_eq!(parse_edid(&[]), Err(EdidError::Truncated));
    }

    #[test]
    fn test_reject_extension_overflow() {
        let mut bytes = [0u8; 256];
        bytes.copy_from_slice(EDID);
        bytes[126] = 9;
        // Re-checksum the base block so only the count is wrong.
        let sum = bytes[..127].iter().fold(0u8, |a, b| a.wrapping_add(*b));
        bytes[127] = 0u8.wrapping_sub(sum);
        assert_eq!(parse_edid(&bytes), Err(EdidError::TooManyExtensions));
    }

    /// Garbage inside a valid frame never panics and never yields a degenerate mode.
    #[test]
    fn test_reject_garbage_descriptors_without_panic() {
        let mut bytes = [0u8; 256];
        bytes.copy_from_slice(EDID);
        for b in &mut bytes[0x36..0x7e] {
            *b = 0xff;
        }
        let sum = bytes[..127].iter().fold(0u8, |a, b| a.wrapping_add(*b));
        bytes[127] = 0u8.wrapping_sub(sum);
        for b in &mut bytes[128 + 4..255] {
            *b = 0xff;
        }
        let sum = bytes[128..255].iter().fold(0u8, |a, b| a.wrapping_add(*b));
        bytes[255] = 0u8.wrapping_sub(sum);
        let e = parse_edid(&bytes).expect("frame still valid");
        for m in e.modes.as_slice() {
            assert!(m.h_active > 0 && m.v_active > 0 && m.pixel_clock_khz > 0);
        }
        assert!(e.modes.len() <= ModeList::CAP);
    }

    #[test]
    fn test_reject_nothing_fits() {
        let e = parse_edid(EDID).expect("edid");
        assert_eq!(pick_mode(e.modes.as_slice(), (320, 200), e.screen_cm), None);
    }
}
