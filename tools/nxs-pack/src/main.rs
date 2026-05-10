// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: System-set packer tool for building signed `.nxs` archives
//! OWNERS: @tools-team
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests
//!
//! PUBLIC API:
//!   - CLI: nxs-pack --input <dir> --meta <toml> --key <hex> --output <file.nxs>
//!
//! DEPENDENCIES:
//!   - capnp: system-set index encoding
//!   - ed25519-dalek: detached signature
//!   - tar: deterministic archive creation
//!   - sha2: SHA-256 digests for bundle binding
//!
//! ADR: docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md

use std::collections::HashSet;
use std::env;
use std::fs::{self, File};
use std::io::{self, Cursor, Write};
use std::path::{Path, PathBuf};

use capnp::message::{Builder, ReaderOptions};
use capnp::serialize;
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use tar::{Builder as TarBuilder, EntryType, Header};
use toml::Value;

use nexus_idl_runtime::manifest_capnp::bundle_manifest;

// Generated Cap'n Proto bindings - allow clippy lints we don't control.
#[allow(clippy::unwrap_used, clippy::needless_lifetimes)]
pub mod system_set_capnp {
    include!(concat!(env!("OUT_DIR"), "/system_set_capnp.rs"));
}

use system_set_capnp::system_set_index;

const MAX_NXS_ARCHIVE_BYTES: u64 = 100 * 1024 * 1024;
const MAX_SYSTEM_NXSINDEX_BYTES: usize = 1024 * 1024;
const MAX_MANIFEST_NXB_BYTES: usize = 256 * 1024;
const MAX_PAYLOAD_ELF_BYTES: usize = 50 * 1024 * 1024;
const MAX_BUNDLES_PER_SET: usize = 256;

struct Meta {
    system_version: String,
    timestamp_unix_ms: u64,
}

struct BundleInput {
    name: String,
    version: String,
    dir_name: String,
    manifest_bytes: Vec<u8>,
    payload_bytes: Vec<u8>,
    manifest_sha256: [u8; 32],
    payload_sha256: [u8; 32],
    payload_size: u64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args()?;
    let meta = parse_meta(&args.meta_path)?;
    let signing_key = load_signing_key(&args.key_path)?;
    let publisher = signing_key.verifying_key().to_bytes();

    let mut bundles = load_bundles(&args.input_dir)?;
    if bundles.is_empty() {
        return Err("no .nxb bundles found in input directory".into());
    }
    if bundles.len() > MAX_BUNDLES_PER_SET {
        return Err(
            format!("too many bundles: {} (max {})", bundles.len(), MAX_BUNDLES_PER_SET).into()
        );
    }
    bundles.sort_by(|a, b| a.name.cmp(&b.name));

    let index_bytes = build_system_index(&meta, &publisher, &bundles)?;
    if index_bytes.len() > MAX_SYSTEM_NXSINDEX_BYTES {
        return Err(format!(
            "system.nxsindex too large: {} bytes (max {})",
            index_bytes.len(),
            MAX_SYSTEM_NXSINDEX_BYTES
        )
        .into());
    }
    let signature = signing_key.sign(&index_bytes);
    let signature_bytes = signature.to_bytes();

    write_archive(&args.output_path, &index_bytes, &signature_bytes, &bundles)?;
    enforce_archive_size(&args.output_path)?;

    Ok(())
}

struct Args {
    input_dir: PathBuf,
    meta_path: PathBuf,
    key_path: PathBuf,
    output_path: PathBuf,
}

fn parse_args() -> Result<Args, Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let mut input_dir = None;
    let mut meta_path = None;
    let mut key_path = None;
    let mut output_path = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" => input_dir = Some(next_arg(&mut args, "--input")?),
            "--meta" => meta_path = Some(next_arg(&mut args, "--meta")?),
            "--key" => key_path = Some(next_arg(&mut args, "--key")?),
            "--output" => output_path = Some(next_arg(&mut args, "--output")?),
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }

    let input_dir = input_dir.ok_or_else(|| usage("missing --input"))?;
    let meta_path = meta_path.ok_or_else(|| usage("missing --meta"))?;
    let key_path = key_path.ok_or_else(|| usage("missing --key"))?;
    let output_path = output_path.ok_or_else(|| usage("missing --output"))?;

    Ok(Args {
        input_dir: PathBuf::from(input_dir),
        meta_path: PathBuf::from(meta_path),
        key_path: PathBuf::from(key_path),
        output_path: PathBuf::from(output_path),
    })
}

fn next_arg(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    args.next().ok_or_else(|| usage(&format!("missing value for {flag}")))
}

fn usage(message: &str) -> Box<dyn std::error::Error> {
    Box::<dyn std::error::Error>::from(message.to_string())
}

fn print_usage() {
    let mut stderr = io::stderr();
    let _ = writeln!(
        stderr,
        "nxs-pack usage:\n  nxs-pack --input <dir> --meta <system-set.toml> --key <ed25519-hex> --output <file.nxs>"
    );
}

fn parse_meta(path: &Path) -> Result<Meta, Box<dyn std::error::Error>> {
    let toml_str = fs::read_to_string(path)?;
    let root: Value = toml::from_str(&toml_str)?;
    let table = root.as_table().ok_or("system-set.toml root must be a table")?;

    let system_version = req_str(table, "system_version")?.trim().to_string();
    if system_version.is_empty() {
        return Err("system_version must not be empty".into());
    }
    let timestamp_unix_ms = opt_u64(table, "timestamp_unix_ms")?.unwrap_or(0);

    Ok(Meta { system_version, timestamp_unix_ms })
}

fn req_str<'a>(
    table: &'a toml::Table,
    key: &'static str,
) -> Result<&'a str, Box<dyn std::error::Error>> {
    table
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("system-set.toml missing/invalid `{key}`").into())
}

fn opt_u64(
    table: &toml::Table,
    key: &'static str,
) -> Result<Option<u64>, Box<dyn std::error::Error>> {
    let Some(v) = table.get(key) else {
        return Ok(None);
    };
    let Some(n) = v.as_integer() else {
        return Err(format!("system-set.toml `{key}` must be an integer").into());
    };
    if n < 0 {
        return Err(format!("system-set.toml `{key}` must be >= 0").into());
    }
    Ok(Some(n as u64))
}

fn load_signing_key(path: &Path) -> Result<SigningKey, Box<dyn std::error::Error>> {
    let key_hex = fs::read_to_string(path)?;
    let key_bytes = hex::decode(key_hex.trim())?;
    if key_bytes.len() != 32 {
        return Err(format!("ed25519 key must be 32 bytes (hex), got {}", key_bytes.len()).into());
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&key_bytes);
    Ok(SigningKey::from_bytes(&seed))
}

fn load_bundles(input_dir: &Path) -> Result<Vec<BundleInput>, Box<dyn std::error::Error>> {
    if !input_dir.is_dir() {
        return Err(format!("input directory not found: {}", input_dir.display()).into());
    }

    let mut bundles = Vec::new();
    let mut seen = HashSet::new();
    for entry in fs::read_dir(input_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(dir_name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !dir_name.ends_with(".nxb") {
            continue;
        }

        let bundle = load_bundle(&path, dir_name)?;
        if !seen.insert(bundle.name.clone()) {
            return Err(format!("duplicate bundle name: {}", bundle.name).into());
        }
        bundles.push(bundle);
    }

    Ok(bundles)
}

fn load_bundle(path: &Path, dir_name: &str) -> Result<BundleInput, Box<dyn std::error::Error>> {
    let manifest_path = path.join("manifest.nxb");
    let payload_path = path.join("payload.elf");
    if !manifest_path.is_file() {
        return Err(format!("missing manifest.nxb in {}", path.display()).into());
    }
    if !payload_path.is_file() {
        return Err(format!("missing payload.elf in {}", path.display()).into());
    }

    let manifest_bytes = read_file_with_limit(&manifest_path, MAX_MANIFEST_NXB_BYTES)?;
    let payload_bytes = read_file_with_limit(&payload_path, MAX_PAYLOAD_ELF_BYTES)?;
    let (name, version) = parse_manifest(&manifest_bytes)?;

    let expected_dir = format!("{name}.nxb");
    if dir_name != expected_dir {
        return Err(format!("bundle dir `{dir_name}` does not match manifest name `{name}`").into());
    }

    let manifest_sha256 = sha256(&manifest_bytes);
    let payload_sha256 = sha256(&payload_bytes);
    let payload_size = payload_bytes.len() as u64;

    Ok(BundleInput {
        name,
        version,
        dir_name: dir_name.to_string(),
        manifest_bytes,
        payload_bytes,
        manifest_sha256,
        payload_sha256,
        payload_size,
    })
}

fn read_file_with_limit(path: &Path, limit: usize) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    if bytes.len() > limit {
        return Err(format!(
            "file {} too large: {} bytes (max {})",
            path.display(),
            bytes.len(),
            limit
        )
        .into());
    }
    Ok(bytes)
}

fn parse_manifest(bytes: &[u8]) -> Result<(String, String), Box<dyn std::error::Error>> {
    let mut cursor = Cursor::new(bytes);
    let message = serialize::read_message(&mut cursor, ReaderOptions::new())
        .map_err(|err| format!("manifest decode error: {err}"))?;
    let m = message
        .get_root::<bundle_manifest::Reader<'_>>()
        .map_err(|err| format!("manifest decode error: {err}"))?;

    let name_raw = m.get_name().map_err(|err| format!("manifest decode error: {err}"))?;
    let name_raw =
        name_raw.to_str().map_err(|err| format!("manifest name invalid utf-8: {err}"))?;
    let name = name_raw.trim().to_string();
    if name.is_empty() {
        return Err("manifest name must not be empty".into());
    }

    let semver_raw = m.get_semver().map_err(|err| format!("manifest decode error: {err}"))?;
    let semver_raw =
        semver_raw.to_str().map_err(|err| format!("manifest semver invalid utf-8: {err}"))?;
    let version = semver_raw.trim().to_string();
    if version.is_empty() {
        return Err("manifest semver must not be empty".into());
    }

    Ok((name, version))
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn build_system_index(
    meta: &Meta,
    publisher: &[u8; 32],
    bundles: &[BundleInput],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut builder = Builder::new_default();
    let mut root = builder.init_root::<system_set_index::Builder>();
    root.set_schema_version(1);
    root.set_system_version(&meta.system_version);
    root.set_publisher(publisher);
    root.set_timestamp_unix_ms(meta.timestamp_unix_ms);

    let mut list = root.reborrow().init_bundles(bundles.len() as u32);
    for (i, bundle) in bundles.iter().enumerate() {
        let mut entry = list.reborrow().get(i as u32);
        entry.set_name(&bundle.name);
        entry.set_version(&bundle.version);
        entry.set_manifest_sha256(&bundle.manifest_sha256);
        entry.set_payload_sha256(&bundle.payload_sha256);
        entry.set_payload_size(bundle.payload_size);
    }

    let mut out = Vec::new();
    capnp::serialize::write_message(&mut out, &builder)?;
    Ok(out)
}

fn write_archive(
    output_path: &Path,
    index_bytes: &[u8],
    signature_bytes: &[u8; 64],
    bundles: &[BundleInput],
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::create(output_path)?;
    let mut builder = TarBuilder::new(file);

    append_file(&mut builder, "system.nxsindex", index_bytes)?;
    append_file(&mut builder, "system.sig.ed25519", signature_bytes)?;

    for bundle in bundles {
        let dir_path = format!("{}/", bundle.dir_name);
        append_dir(&mut builder, &dir_path)?;

        let manifest_path = format!("{}/manifest.nxb", bundle.dir_name);
        append_file(&mut builder, &manifest_path, &bundle.manifest_bytes)?;

        let payload_path = format!("{}/payload.elf", bundle.dir_name);
        append_file(&mut builder, &payload_path, &bundle.payload_bytes)?;
    }

    builder.finish()?;
    Ok(())
}

fn append_file(
    builder: &mut TarBuilder<File>,
    path: &str,
    bytes: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Regular);
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();
    builder.append_data(&mut header, path, bytes)?;
    Ok(())
}

fn append_dir(
    builder: &mut TarBuilder<File>,
    path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Directory);
    header.set_size(0);
    header.set_mode(0o755);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();
    builder.append_data(&mut header, path, io::empty())?;
    Ok(())
}

fn enforce_archive_size(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let size = fs::metadata(path)?.len();
    if size > MAX_NXS_ARCHIVE_BYTES {
        let _ = fs::remove_file(path);
        return Err(format!(
            "output archive too large: {} bytes (max {})",
            size, MAX_NXS_ARCHIVE_BYTES
        )
        .into());
    }
    Ok(())
}
