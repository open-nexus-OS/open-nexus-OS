// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Wire-format pin tests for the serialized `CommandBuffer` (windowd↔gpud
//! contract) — split out of `buffer.rs` (structure-gate).

#[cfg(test)]
mod scroll_tag_tests {
    use crate::command::buffer::*;

    /// The scroll fast path depends on the `scroll_id` surviving the wire to
    /// gpud. This pins that: a scrollable CompositeLayer serialized + deserialized
    /// keeps its id (and a normal one stays 0). If this breaks, gpud can't
    /// identify scrollable layers and the scroll fast path silently dies.
    #[test]
    fn scroll_id_roundtrips_over_the_wire() {
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
                    scroll_id: 1,
                    content_w: 200,
                    content_h: 300,
                    scroll_band_top_abs: 3200,
                    scroll_band_h: 2600,
                    layer_id: 0,
                    content_epoch: 7,
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
                    scroll_id: 0,
                    content_w: 0,
                    content_h: 0,
                    scroll_band_top_abs: 0,
                    scroll_band_h: 0,
                    layer_id: 0,
                    content_epoch: 0,
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
                    scroll_id,
                    content_w,
                    content_epoch,
                    ..
                } => Some((*src_row_abs, *scroll_id, *content_w, *content_epoch)),
                _ => None,
            })
            .collect();
        // scroll_id, the content sub-size AND the content epoch survive the
        // wire (windowd↔gpud) — the epoch gates the per-frame atlas upload.
        assert_eq!(
            tags,
            alloc::vec![(3200, 1, 200, 7), (4000, 0, 0, 0)],
            "scroll_id + content sub-size + content epoch preserved per layer"
        );
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
    /// before any display sees it.
    #[test]
    fn test_reject_damage_empty_or_wrapping() {
        use crate::core::types::TileRect;
        for rect in [
            TileRect { x: 0, y: 0, width: 0, height: 8 },
            TileRect { x: u32::MAX, y: 0, width: 2, height: 2 },
        ] {
            let cb = CommittedBuffer { commands: alloc::vec![Command::Damage { rect, origin: 0 }] };
            let mut buf = [0u8; 32];
            let n = cb.serialize_into(&mut buf).expect("serialize");
            assert!(CommittedBuffer::deserialize_from(&buf[..n]).is_err(), "{rect:?}");
        }
    }
}
