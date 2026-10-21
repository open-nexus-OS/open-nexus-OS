// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Wire-format pin tests for the serialized `CommandBuffer` (windowd↔gpud
//! contract) — split out of `buffer.rs` (structure-gate).

#[cfg(test)]
mod scroll_tag_tests {
    use crate::command::buffer::*;

    /// A scrolled body depends on its band surviving the wire: a backend that uploads the
    /// atlas uploads the whole band once, so the present that scrolls (a new `src_row_abs`)
    /// samples rows already there. This pins the band, the content sub-size and the content
    /// epoch per layer (a plain layer keeps zeros).
    #[test]
    fn scroll_band_roundtrips_over_the_wire() {
        let cb = CommittedBuffer {
            commands: alloc::vec![
                Command::CompositeLayer {
                    src_row_abs: 3200,
                    src_x: 0,
                    width: 366,
                    height: 600,
                    dst_x: 100,
                    dst_y: 80,
                    opacity: 255,
                    corner_radius: 18,
                    shadow_blur: 24,
                    shadow_offset_y: 12,
                    shadow_alpha: 180,
                    backdrop_blur: 0,
                    content_w: 200,
                    content_h: 300,
                    scroll_band_top_abs: 3200,
                    scroll_band_h: 2600,
                    layer_id: 0,
                    content_epoch: 7,
                    flags: 0,
                },
                Command::CompositeLayer {
                    src_row_abs: 4000,
                    src_x: 0,
                    width: 64,
                    height: 800,
                    dst_x: 0,
                    dst_y: 0,
                    opacity: 255,
                    corner_radius: 0,
                    shadow_blur: 0,
                    shadow_offset_y: 0,
                    shadow_alpha: 0,
                    backdrop_blur: 0,
                    content_w: 0,
                    content_h: 0,
                    scroll_band_top_abs: 0,
                    scroll_band_h: 0,
                    layer_id: 0,
                    content_epoch: 0,
                    flags: 0,
                },
            ],
        };
        let mut buf = [0u8; 256];
        let n = cb.serialize_into(&mut buf).expect("serialize");
        let (out, consumed) = CommittedBuffer::deserialize_from(&buf[..n]).expect("deserialize");
        assert_eq!(consumed, n, "consumed all serialized bytes");
        let tags: alloc::vec::Vec<(u32, u32, u32, u32)> = out
            .commands
            .iter()
            .filter_map(|c| match c {
                Command::CompositeLayer {
                    src_row_abs,
                    scroll_band_h,
                    content_w,
                    content_epoch,
                    ..
                } => Some((*src_row_abs, *scroll_band_h, *content_w, *content_epoch)),
                _ => None,
            })
            .collect();
        // The band, the content sub-size AND the content epoch survive the wire
        // (windowd↔gpud) — the epoch gates the atlas upload.
        assert_eq!(
            tags,
            alloc::vec![(3200, 2600, 200, 7), (4000, 0, 0, 0)],
            "scroll band + content sub-size + content epoch preserved per layer"
        );
        assert_eq!(out.commands, cb.commands, "every word as sent");
    }

    /// The scene's damage (RFC-0101 §8) crosses the wire as its own command, beside the
    /// pointer's blend that shares its four words.
    #[test]
    fn damage_roundtrips_over_the_wire() {
        use crate::core::types::TileRect;
        let rect = TileRect { x: 12, y: 34, width: 56, height: 78 };
        let cb = CommittedBuffer {
            commands: alloc::vec![
                Command::Damage { rect, origin: 3 },
                Command::BlendCursor { x: 1, y: 2, width: 3, height: 4 },
            ],
        };
        let mut buf = [0u8; 64];
        let n = cb.serialize_into(&mut buf).expect("serialize");
        let (out, consumed) = CommittedBuffer::deserialize_from(&buf[..n]).expect("deserialize");
        assert_eq!(consumed, n);
        assert_eq!(out.commands, cb.commands);
    }

    /// A damage rectangle that is empty or wraps the coordinate space is refused at decode,
    /// before any display sees it — the one empty rectangle accepted is `TileRect::NONE` from the
    /// base (a present that names no damage); the same rectangle claimed by a window is not.
    #[test]
    fn test_reject_damage_empty_or_wrapping() {
        use crate::core::types::TileRect;
        for (rect, origin) in [
            (TileRect { x: 0, y: 0, width: 0, height: 8 }, 0),
            (TileRect { x: 5, y: 9, width: 0, height: 0 }, 0),
            (TileRect::NONE, 2),
            (TileRect { x: u32::MAX, y: 0, width: 2, height: 2 }, 0),
        ] {
            let cb = CommittedBuffer { commands: alloc::vec![Command::Damage { rect, origin }] };
            let mut buf = [0u8; 32];
            let n = cb.serialize_into(&mut buf).expect("serialize");
            assert!(CommittedBuffer::deserialize_from(&buf[..n]).is_err(), "{rect:?} {origin}");
        }
    }

    /// A present that names no damage (TASK-0338: owed for what moved alone) crosses the wire.
    #[test]
    fn no_damage_roundtrips_over_the_wire() {
        use crate::core::types::TileRect;
        let cb = CommittedBuffer {
            commands: alloc::vec![Command::Damage { rect: TileRect::NONE, origin: 0 }],
        };
        let mut buf = [0u8; 32];
        let n = cb.serialize_into(&mut buf).expect("serialize");
        let (out, consumed) = CommittedBuffer::deserialize_from(&buf[..n]).expect("deserialize");
        assert_eq!(consumed, n);
        assert_eq!(out.commands, cb.commands);
    }

    /// A window's transform crosses the wire with the present (TASK-0336): a negative offset,
    /// a fade, a scale — every word as sent.
    #[test]
    fn layer_transform_roundtrips_over_the_wire() {
        let cb = CommittedBuffer {
            commands: alloc::vec![
                Command::LayerTransform {
                    layer_id: 2,
                    dx: -40,
                    dy: 17,
                    opacity: 128,
                    scale_pct: 92
                },
                Command::LayerTransform { layer_id: 3, dx: 0, dy: 0, opacity: 255, scale_pct: 100 },
            ],
        };
        let mut buf = [0u8; 64];
        let n = cb.serialize_into(&mut buf).expect("serialize");
        let (out, consumed) = CommittedBuffer::deserialize_from(&buf[..n]).expect("deserialize");
        assert_eq!(consumed, n);
        assert_eq!(out.commands, cb.commands);
    }

    /// A transform that names no window, an opacity past full or a scale of nothing or past
    /// four times is refused at decode.
    #[test]
    fn test_reject_a_transform_for_no_window_or_out_of_range() {
        for (layer_id, opacity, scale_pct) in
            [(0, 255, 100), (1, 256, 100), (1, 255, 0), (1, 255, 401)]
        {
            let cb = CommittedBuffer {
                commands: alloc::vec![Command::LayerTransform {
                    layer_id,
                    dx: 0,
                    dy: 0,
                    opacity,
                    scale_pct
                }],
            };
            let mut buf = [0u8; 32];
            let n = cb.serialize_into(&mut buf).expect("serialize");
            assert!(
                CommittedBuffer::deserialize_from(&buf[..n]).is_err(),
                "{layer_id} {opacity} {scale_pct}"
            );
        }
    }

    /// The layer's flags (TASK-0337: `LAYER_OPAQUE`, the occluder) cross the wire.
    #[test]
    fn composite_layer_flags_roundtrip_over_the_wire() {
        let layer = |flags| Command::CompositeLayer {
            src_row_abs: 3200,
            src_x: 0,
            width: 400,
            height: 300,
            dst_x: 10,
            dst_y: 20,
            opacity: 255,
            corner_radius: 12,
            shadow_blur: 0,
            shadow_offset_y: 0,
            shadow_alpha: 0,
            backdrop_blur: 0,
            content_w: 0,
            content_h: 0,
            scroll_band_top_abs: 0,
            scroll_band_h: 0,
            layer_id: 1,
            content_epoch: 4,
            flags,
        };
        let cb = CommittedBuffer {
            commands: alloc::vec![layer(super::super::buffer::LAYER_OPAQUE), layer(0)],
        };
        let mut buf = [0u8; 256];
        let n = cb.serialize_into(&mut buf).expect("serialize");
        let (out, _) = CommittedBuffer::deserialize_from(&buf[..n]).expect("deserialize");
        assert_eq!(out.commands, cb.commands);
    }
}
