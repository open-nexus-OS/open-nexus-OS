// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The card clock divider (SDHCI 3.00 §2.2.14): from specification 3.00 on a 10-bit divided
//! clock (`base / 2N`, `N = 0` is the base itself), before it a power of two up to 256.
//! Always the fastest clock not above the target — a card is never overclocked.
//! OWNERS: @runtime @drivers

use crate::regs::SPEC_300;

/// The largest 10-bit divider.
const MAX_N: u64 = 1023;
/// The largest power-of-two divisor before 3.00.
const MAX_POW2: u64 = 256;

/// The clock-control divider bits for the fastest card clock not above `target_hz`, and
/// that clock in Hz (rounded down); `None` when even the largest divider is too fast or an
/// input is zero.
pub fn divider(version: u8, base_hz: u32, target_hz: u32) -> Option<(u32, u32)> {
    let (base, target) = (u64::from(base_hz), u64::from(target_hz));
    if base == 0 || target == 0 {
        return None;
    }
    if version >= SPEC_300 {
        let n = if base <= target { 0 } else { base.div_ceil(2 * target) };
        if n > MAX_N {
            return None;
        }
        let actual = if n == 0 { base } else { base / (2 * n) };
        let bits = ((n & 0xFF) << 8) | (((n >> 8) & 0x3) << 6);
        Some((bits as u32, actual as u32))
    } else {
        let mut d = 1u64;
        while base > target * d {
            d *= 2;
            if d > MAX_POW2 {
                return None;
            }
        }
        Some((((d / 2) << 8) as u32, (base / d) as u32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const K1_IO: u32 = 375_000_000;

    #[test]
    fn the_k1_io_clock_divides_to_the_measured_operating_points() {
        // Identification, legacy, high speed, and HS400: 187.5 MHz is what the stock system
        // runs (`/sys/kernel/debug/mmc2/ios`, the 2026-09-24 measurement).
        assert_eq!(
            divider(SPEC_300, K1_IO, 400_000),
            Some((((469 & 0xFF) << 8) | (1 << 6), 399_786))
        );
        assert_eq!(divider(SPEC_300, K1_IO, 26_000_000), Some((8 << 8, 23_437_500)));
        assert_eq!(divider(SPEC_300, K1_IO, 52_000_000), Some((4 << 8, 46_875_000)));
        assert_eq!(divider(SPEC_300, K1_IO, 200_000_000), Some((1 << 8, 187_500_000)));
    }

    #[test]
    fn a_spec_3_host_at_52_mhz_hits_400_khz_exactly_and_runs_high_speed_undivided() {
        assert_eq!(divider(SPEC_300, 52_000_000, 400_000), Some((65 << 8, 400_000)));
        assert_eq!(divider(SPEC_300, 52_000_000, 52_000_000), Some((0, 52_000_000)));
        assert_eq!(divider(SPEC_300, 52_000_000, 200_000_000), Some((0, 52_000_000)));
    }

    #[test]
    fn a_spec_2_host_divides_by_powers_of_two_and_never_overclocks() {
        // 52 MHz / 128 = 406 kHz is above 400 kHz: the divisor is 256.
        assert_eq!(divider(1, 52_000_000, 400_000), Some((128 << 8, 203_125)));
        assert_eq!(divider(1, 52_000_000, 26_000_000), Some((1 << 8, 26_000_000)));
        assert_eq!(divider(1, 52_000_000, 52_000_000), Some((0, 52_000_000)));
    }

    #[test]
    fn test_reject_a_clock_the_divider_cannot_reach() {
        assert_eq!(divider(SPEC_300, K1_IO, 0), None);
        assert_eq!(divider(SPEC_300, 0, 400_000), None);
        // 2 × 1023 × 100 kHz = 204.6 MHz: a 1 GHz base cannot come down to 100 kHz.
        assert_eq!(divider(SPEC_300, 1_000_000_000, 100_000), None);
        assert_eq!(divider(1, 208_000_000, 400_000), None);
    }

    #[test]
    fn every_divided_clock_is_at_or_below_its_target() {
        for version in [1u8, SPEC_300] {
            for base in [25_000_000u32, 52_000_000, 100_000_000, 200_000_000, K1_IO] {
                for target in [400_000u32, 20_000_000, 26_000_000, 52_000_000, 200_000_000] {
                    if let Some((_, actual)) = divider(version, base, target) {
                        assert!(
                            actual <= target && actual > 0,
                            "{version} {base} {target} {actual}"
                        );
                    }
                }
            }
        }
    }
}
