// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Deterministic minimal PNG encode/decode for host snapshot artifacts.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 24 ui_host_snap integration tests
//! ADR: docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md

use crate::codec::{SnapResult, SnapshotError};

use ui_renderer::BYTES_PER_PIXEL;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedPng {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn encode_png_rgba(width: u32, height: u32, rgba: &[u8]) -> SnapResult<Vec<u8>> {
    let row_bytes = usize::try_from(
        u64::from(width)
            .checked_mul(u64::from(BYTES_PER_PIXEL))
            .ok_or(SnapshotError::ImageDataInvalid)?,
    )
    .map_err(|_| SnapshotError::ImageDataInvalid)?;
    let height_usize = usize::try_from(height).map_err(|_| SnapshotError::ImageDataInvalid)?;
    if rgba.len()
        != row_bytes
            .checked_mul(height_usize)
            .ok_or(SnapshotError::ImageDataInvalid)?
    {
        return Err(SnapshotError::ImageDataInvalid);
    }

    let mut scanlines = Vec::with_capacity(
        row_bytes
            .checked_add(1)
            .and_then(|row| row.checked_mul(height_usize))
            .ok_or(SnapshotError::ImageDataInvalid)?,
    );
    for row in 0..height_usize {
        scanlines.push(0);
        let start = row
            .checked_mul(row_bytes)
            .ok_or(SnapshotError::ImageDataInvalid)?;
        scanlines.extend_from_slice(&rgba[start..start + row_bytes]);
    }

    let mut out = Vec::new();
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    append_png_chunk(&mut out, *b"IHDR", &ihdr)?;

    let mut zlib = Vec::new();
    zlib.extend_from_slice(&[0x78, 0x01]);
    append_stored_deflate_blocks(&mut zlib, &scanlines)?;
    zlib.extend_from_slice(&adler32(&scanlines).to_be_bytes());
    append_png_chunk(&mut out, *b"IDAT", &zlib)?;
    append_png_chunk(&mut out, *b"IEND", &[])?;
    Ok(out)
}

pub fn decode_png_rgba(input: &[u8]) -> SnapResult<DecodedPng> {
    if input.len() < 8 || &input[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err(SnapshotError::Codec);
    }
    let mut pos = 8usize;
    let mut width = None;
    let mut height = None;
    let mut idat = Vec::new();
    while pos < input.len() {
        let length = read_be_u32(input, pos)? as usize;
        pos = pos.checked_add(4).ok_or(SnapshotError::Codec)?;
        let chunk_type = input.get(pos..pos + 4).ok_or(SnapshotError::Codec)?;
        pos = pos.checked_add(4).ok_or(SnapshotError::Codec)?;
        let data = input.get(pos..pos + length).ok_or(SnapshotError::Codec)?;
        pos = pos.checked_add(length).ok_or(SnapshotError::Codec)?;
        let _crc = input.get(pos..pos + 4).ok_or(SnapshotError::Codec)?;
        pos = pos.checked_add(4).ok_or(SnapshotError::Codec)?;

        match chunk_type {
            b"IHDR" => {
                if data.len() != 13 || data[8] != 8 || data[9] != 6 || data[12] != 0 {
                    return Err(SnapshotError::Codec);
                }
                width = Some(read_be_u32(data, 0)?);
                height = Some(read_be_u32(data, 4)?);
            }
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
    }

    let width = width.ok_or(SnapshotError::Codec)?;
    let height = height.ok_or(SnapshotError::Codec)?;
    let row_bytes = usize::try_from(
        u64::from(width)
            .checked_mul(u64::from(BYTES_PER_PIXEL))
            .ok_or(SnapshotError::Codec)?,
    )
    .map_err(|_| SnapshotError::Codec)?;
    let height_usize = usize::try_from(height).map_err(|_| SnapshotError::Codec)?;
    let inflated = decode_zlib_stored(&idat)?;
    let expected = row_bytes
        .checked_add(1)
        .and_then(|row| row.checked_mul(height_usize))
        .ok_or(SnapshotError::Codec)?;
    if inflated.len() != expected {
        return Err(SnapshotError::Codec);
    }
    let mut rgba = Vec::with_capacity(
        row_bytes
            .checked_mul(height_usize)
            .ok_or(SnapshotError::Codec)?,
    );
    for row in 0..height_usize {
        let start = row.checked_mul(row_bytes + 1).ok_or(SnapshotError::Codec)?;
        if inflated[start] != 0 {
            return Err(SnapshotError::Codec);
        }
        rgba.extend_from_slice(&inflated[start + 1..start + 1 + row_bytes]);
    }
    Ok(DecodedPng {
        width,
        height,
        rgba,
    })
}

pub fn insert_chunk_after_ihdr(
    input: &[u8],
    chunk_type: [u8; 4],
    data: &[u8],
) -> SnapResult<Vec<u8>> {
    if input.len() < 33 || &input[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err(SnapshotError::Codec);
    }
    let ihdr_len = read_be_u32(input, 8)? as usize;
    let ihdr_end = 8usize
        .checked_add(4)
        .and_then(|value| value.checked_add(4))
        .and_then(|value| value.checked_add(ihdr_len))
        .and_then(|value| value.checked_add(4))
        .ok_or(SnapshotError::Codec)?;
    let mut out = Vec::new();
    out.extend_from_slice(input.get(..ihdr_end).ok_or(SnapshotError::Codec)?);
    append_png_chunk(&mut out, chunk_type, data)?;
    out.extend_from_slice(input.get(ihdr_end..).ok_or(SnapshotError::Codec)?);
    Ok(out)
}

fn append_png_chunk(out: &mut Vec<u8>, chunk_type: [u8; 4], data: &[u8]) -> SnapResult<()> {
    let len = u32::try_from(data.len()).map_err(|_| SnapshotError::Codec)?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&chunk_type);
    out.extend_from_slice(data);
    let crc = crc32(&chunk_type, data);
    out.extend_from_slice(&crc.to_be_bytes());
    Ok(())
}

fn append_stored_deflate_blocks(out: &mut Vec<u8>, data: &[u8]) -> SnapResult<()> {
    let mut offset = 0usize;
    while offset < data.len() {
        let remaining = data.len() - offset;
        let len = remaining.min(65_535);
        let final_block = if offset + len == data.len() { 1u8 } else { 0u8 };
        out.push(final_block);
        let len_u16 = u16::try_from(len).map_err(|_| SnapshotError::Codec)?;
        out.extend_from_slice(&len_u16.to_le_bytes());
        out.extend_from_slice(&(!len_u16).to_le_bytes());
        out.extend_from_slice(&data[offset..offset + len]);
        offset += len;
    }
    if data.is_empty() {
        out.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    Ok(())
}

fn decode_zlib_stored(input: &[u8]) -> SnapResult<Vec<u8>> {
    if input.len() < 6 {
        return Err(SnapshotError::Codec);
    }
    let mut pos = 2usize;
    let end_without_adler = input.len() - 4;
    let mut out = Vec::new();
    loop {
        if pos >= end_without_adler {
            return Err(SnapshotError::Codec);
        }
        let header = input[pos];
        pos += 1;
        let is_final = (header & 1) == 1;
        let block_type = (header >> 1) & 0b11;
        if block_type != 0 {
            return Err(SnapshotError::Codec);
        }
        let len = read_le_u16(input, pos)?;
        pos += 2;
        let nlen = read_le_u16(input, pos)?;
        pos += 2;
        if nlen != !len {
            return Err(SnapshotError::Codec);
        }
        let len_usize = usize::from(len);
        let data = input
            .get(pos..pos + len_usize)
            .ok_or(SnapshotError::Codec)?;
        out.extend_from_slice(data);
        pos += len_usize;
        if is_final {
            break;
        }
    }
    if adler32(&out).to_be_bytes() != input[end_without_adler..] {
        return Err(SnapshotError::Codec);
    }
    Ok(out)
}

fn read_be_u32(input: &[u8], pos: usize) -> SnapResult<u32> {
    let bytes = input.get(pos..pos + 4).ok_or(SnapshotError::Codec)?;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_le_u16(input: &[u8], pos: usize) -> SnapResult<u16> {
    let bytes = input.get(pos..pos + 2).ok_or(SnapshotError::Codec)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn adler32(data: &[u8]) -> u32 {
    const MOD_ADLER: u32 = 65_521;
    let mut a = 1u32;
    let mut b = 0u32;
    for byte in data {
        a = (a + u32::from(*byte)) % MOD_ADLER;
        b = (b + a) % MOD_ADLER;
    }
    (b << 16) | a
}

fn crc32(chunk_type: &[u8; 4], data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in chunk_type.iter().chain(data.iter()) {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = if (crc & 1) == 1 { 0xedb8_8320 } else { 0 };
            crc = (crc >> 1) ^ mask;
        }
    }
    !crc
}
