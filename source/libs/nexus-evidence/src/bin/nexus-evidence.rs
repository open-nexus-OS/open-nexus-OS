// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: CLI binary for the `nexus-evidence` host crate (P5-02 +
//! P5-03). Thin wrapper around the library API: assembles unsigned
//! bundles from a finished QEMU run, inspects existing bundles,
//! prints the canonical hash, and (P5-03) seals existing bundles
//! in-place + verifies signatures against a public key.
//!
//! Argument style matches `nexus-proof-manifest`:
//! `<subcommand> [--key=value]...`. Hand-rolled parser, no `clap`,
//! to keep the host-tool dependency footprint minimal.
//!
//! OWNERS: @runtime
//! STATUS: Functional (P5-02 surface)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: see `tests/assemble.rs` (5 tests; CLI smoke
//!                covered by P5-03 once `seal` lands)
//!
//! Subcommands:
//!
//! ```text
//!   assemble       --uart=<path> --manifest=<path> --profile=<name>
//!                  --out=<path>
//!                  [--kernel-cmdline=<str>] [--qemu-arg=<a> ...]
//!                  [--host-info=<str>] [--build-sha=<sha>]
//!                  [--rustc-version=<str>] [--qemu-version=<str>]
//!                  [--wall-clock=<rfc3339>]
//!                  [--env=<KEY=VALUE> ...]
//!   inspect        <bundle.tar.gz>
//!   canonical-hash <bundle.tar.gz>
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nexus_evidence::{
    canonical_hash, read_unsigned, AssembleOpts, Bundle, EvidenceError, GatherOpts, KeyLabel,
    SigningKey, VerifyingKey,
};

const USAGE: &str = "\
nexus-evidence <subcommand> [options]

subcommands:
  assemble       --uart=<path> --manifest=<path> --profile=<name>
                 --out=<path>
                 [--kernel-cmdline=<str>] [--qemu-arg=<a>]...
                 [--host-info=<str>] [--build-sha=<sha>]
                 [--rustc-version=<str>] [--qemu-version=<str>]
                 [--wall-clock=<rfc3339>] [--env=<KEY=VALUE>]...
  inspect        <bundle.tar.gz>
  canonical-hash <bundle.tar.gz>
  seal           <bundle.tar.gz> --privkey=<path> --label=ci|bringup
                 (P5-04: refuses to seal if the secret scanner trips
                  on uart.log / trace.jsonl / config.json — see
                  docs/testing/evidence-bundle.md §3d)
  verify         <bundle.tar.gz> --pubkey=<path> [--policy=ci|bringup|any]
  keygen         --seed=<hex32bytes> --pubkey-out=<path> [--privkey-out=<path>]

exit codes: 0 ok | 1 schema/usage/IO/signature/secret-leak failure | 2 missing input file
";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let sub = match args.next() {
        Some(s) => s,
        None => {
            eprintln!("{}", USAGE);
            return ExitCode::from(1);
        }
    };
    let rest: Vec<String> = args.collect();

    let result = match sub.as_str() {
        "assemble" => cmd_assemble(&rest),
        "inspect" => cmd_inspect(&rest),
        "canonical-hash" => cmd_canonical_hash(&rest),
        "seal" => cmd_seal(&rest),
        "verify" => cmd_verify(&rest),
        "keygen" => cmd_keygen(&rest),
        "-h" | "--help" | "help" => {
            print!("{}", USAGE);
            return ExitCode::from(0);
        }
        other => {
            eprintln!(
                "nexus-evidence: unknown subcommand `{}`\n\n{}",
                other, USAGE
            );
            return ExitCode::from(1);
        }
    };

    match result {
        Ok(()) => ExitCode::from(0),
        Err(CliError::Usage(msg)) => {
            eprintln!("usage error: {}\n\n{}", msg, USAGE);
            ExitCode::from(1)
        }
        Err(CliError::Io { code, msg }) => {
            eprintln!("nexus-evidence: {}", msg);
            ExitCode::from(code)
        }
        Err(CliError::Evidence(e)) => {
            eprintln!("nexus-evidence: {}", e);
            ExitCode::from(1)
        }
    }
}

#[derive(Debug)]
enum CliError {
    Usage(String),
    Io { code: u8, msg: String },
    Evidence(EvidenceError),
}

impl From<EvidenceError> for CliError {
    fn from(e: EvidenceError) -> Self {
        CliError::Evidence(e)
    }
}

// ---------------------------------------------------------------------------
// Subcommand handlers
// ---------------------------------------------------------------------------

fn cmd_assemble(rest: &[String]) -> Result<(), CliError> {
    let mut uart: Option<PathBuf> = None;
    let mut manifest: Option<PathBuf> = None;
    let mut profile: Option<String> = None;
    let mut out: Option<PathBuf> = None;
    let mut kernel_cmdline = String::new();
    let mut qemu_args: Vec<String> = Vec::new();
    let mut host_info = String::new();
    let mut build_sha = String::new();
    let mut rustc_version = String::new();
    let mut qemu_version = String::new();
    let mut wall_clock = String::new();
    let mut env: BTreeMap<String, String> = BTreeMap::new();

    for arg in rest {
        let (k, v) = split_kv(arg)?;
        match k {
            "--uart" => uart = Some(PathBuf::from(v)),
            "--manifest" => manifest = Some(PathBuf::from(v)),
            "--profile" => profile = Some(v.into()),
            "--out" => out = Some(PathBuf::from(v)),
            "--kernel-cmdline" => kernel_cmdline = v.into(),
            "--qemu-arg" => qemu_args.push(v.into()),
            "--host-info" => host_info = v.into(),
            "--build-sha" => build_sha = v.into(),
            "--rustc-version" => rustc_version = v.into(),
            "--qemu-version" => qemu_version = v.into(),
            "--wall-clock" => wall_clock = v.into(),
            "--env" => {
                let (ek, ev) = v.split_once('=').ok_or_else(|| {
                    CliError::Usage(format!("--env expects KEY=VALUE, got `{}`", v))
                })?;
                env.insert(ek.to_string(), ev.to_string());
            }
            other => return Err(CliError::Usage(format!("unknown flag `{}`", other))),
        }
    }

    let uart = uart.ok_or_else(|| CliError::Usage("--uart=<path> required".into()))?;
    let manifest = manifest.ok_or_else(|| CliError::Usage("--manifest=<path> required".into()))?;
    let profile = profile.ok_or_else(|| CliError::Usage("--profile=<name> required".into()))?;
    let out = out.ok_or_else(|| CliError::Usage("--out=<path> required".into()))?;

    let opts = AssembleOpts {
        uart_path: uart,
        manifest_path: manifest,
        gather_opts: GatherOpts {
            profile: profile.clone(),
            env,
            kernel_cmdline,
            qemu_args,
            host_info,
            build_sha,
            rustc_version,
            qemu_version,
            wall_clock_utc: wall_clock,
        },
    };

    let bundle = Bundle::assemble(opts)?;
    bundle.write_unsigned(&out)?;
    println!("nexus-evidence: wrote unsigned bundle to {}", out.display());
    Ok(())
}

fn cmd_inspect(rest: &[String]) -> Result<(), CliError> {
    let path = single_positional("inspect", rest)?;
    let bundle = read_unsigned(&path).map_err(io_or_evidence("inspect"))?;
    print!("{}", bundle.summary());
    Ok(())
}

fn cmd_canonical_hash(rest: &[String]) -> Result<(), CliError> {
    let path = single_positional("canonical-hash", rest)?;
    let bundle = read_unsigned(&path).map_err(io_or_evidence("canonical-hash"))?;
    println!("{}", hex::encode(canonical_hash(&bundle)));
    Ok(())
}

fn cmd_seal(rest: &[String]) -> Result<(), CliError> {
    let (path, opts) = positional_with_kv("seal", rest)?;
    let mut privkey: Option<PathBuf> = None;
    let mut label: Option<KeyLabel> = None;
    for (k, v) in &opts {
        match k.as_str() {
            "--privkey" => privkey = Some(PathBuf::from(v)),
            "--label" => label = Some(KeyLabel::parse(v).map_err(CliError::Evidence)?),
            other => return Err(CliError::Usage(format!("unknown flag `{}`", other))),
        }
    }
    let privkey = privkey.ok_or_else(|| CliError::Usage("--privkey=<path> required".into()))?;
    let label = label.ok_or_else(|| CliError::Usage("--label=ci|bringup required".into()))?;

    let bundle = read_unsigned(&path).map_err(io_or_evidence("seal"))?;
    let signing_key = read_signing_key(&privkey)?;
    let sealed = bundle.seal(&signing_key, label)?;
    sealed.write_unsigned(&path)?;
    println!(
        "nexus-evidence: sealed bundle in-place at {} (label={})",
        path.display(),
        label.as_str()
    );
    Ok(())
}

fn cmd_verify(rest: &[String]) -> Result<(), CliError> {
    let (path, opts) = positional_with_kv("verify", rest)?;
    let mut pubkey: Option<PathBuf> = None;
    let mut policy: Option<KeyLabel> = None;
    for (k, v) in &opts {
        match k.as_str() {
            "--pubkey" => pubkey = Some(PathBuf::from(v)),
            "--policy" => {
                policy = match v.as_str() {
                    "any" => None,
                    other => Some(KeyLabel::parse(other).map_err(CliError::Evidence)?),
                };
            }
            other => return Err(CliError::Usage(format!("unknown flag `{}`", other))),
        }
    }
    let pubkey = pubkey.ok_or_else(|| CliError::Usage("--pubkey=<path> required".into()))?;
    let bundle = read_unsigned(&path).map_err(io_or_evidence("verify"))?;
    let verifying_key = read_verifying_key(&pubkey)?;
    bundle.verify(&verifying_key, policy)?;
    println!(
        "nexus-evidence: verify ok: {} (label={}, policy={})",
        path.display(),
        bundle
            .signature
            .as_ref()
            .map(|s| s.label.as_str())
            .unwrap_or("?"),
        policy.map(|p| p.as_str()).unwrap_or("any"),
    );
    Ok(())
}

fn cmd_keygen(rest: &[String]) -> Result<(), CliError> {
    let mut seed_hex: Option<String> = None;
    let mut pubkey_out: Option<PathBuf> = None;
    let mut privkey_out: Option<PathBuf> = None;
    for arg in rest {
        let (k, v) = split_kv(arg)?;
        match k {
            "--seed" => seed_hex = Some(v.to_string()),
            "--pubkey-out" => pubkey_out = Some(PathBuf::from(v)),
            "--privkey-out" => privkey_out = Some(PathBuf::from(v)),
            other => return Err(CliError::Usage(format!("unknown flag `{}`", other))),
        }
    }
    let seed_hex =
        seed_hex.ok_or_else(|| CliError::Usage("--seed=<hex32bytes> required".into()))?;
    let pubkey_out =
        pubkey_out.ok_or_else(|| CliError::Usage("--pubkey-out=<path> required".into()))?;
    let raw = hex::decode(seed_hex.trim())
        .map_err(|e| CliError::Usage(format!("--seed hex decode: {}", e)))?;
    if raw.len() != 32 {
        return Err(CliError::Usage(format!(
            "--seed must decode to 32 bytes, got {}",
            raw.len()
        )));
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&raw);
    let signing = SigningKey::from_seed(seed);
    let verifying = signing.verifying_key();

    let pub_hex = hex::encode(verifying.to_bytes());
    std::fs::write(&pubkey_out, format!("{}\n", pub_hex)).map_err(|e| CliError::Io {
        code: 1,
        msg: format!("write pubkey {}: {}", pubkey_out.display(), e),
    })?;
    if let Some(priv_path) = privkey_out {
        let priv_hex = hex::encode(seed);
        std::fs::write(&priv_path, format!("{}\n", priv_hex)).map_err(|e| CliError::Io {
            code: 1,
            msg: format!("write privkey {}: {}", priv_path.display(), e),
        })?;
        eprintln!(
            "nexus-evidence: WARNING wrote private seed to {} (chmod 0600 yourself; never commit)",
            priv_path.display()
        );
    }
    println!(
        "nexus-evidence: wrote pubkey to {} ({} bytes hex)",
        pubkey_out.display(),
        pub_hex.len()
    );
    Ok(())
}

/// Read a 32-byte Ed25519 seed from `path`. Accepted encodings:
///   - 32 raw bytes (no encoding).
///   - hex (64 ASCII chars, optional surrounding whitespace).
///   - base64 standard (44 chars w/ padding, optional whitespace).
///
/// The on-disk format produced by `tools/gen-bringup-key.sh`
/// (P5-04) is one of these; this entry point accepts all so the
/// CLI can operate against any of them without configuration.
fn read_signing_key(path: &Path) -> Result<SigningKey, CliError> {
    let bytes = std::fs::read(path).map_err(|e| CliError::Io {
        code: 2,
        msg: format!("read privkey {}: {}", path.display(), e),
    })?;
    let seed = decode_key_bytes(&bytes, 32, "privkey")?;
    let mut seed_arr = [0u8; 32];
    seed_arr.copy_from_slice(&seed);
    Ok(SigningKey::from_seed(seed_arr))
}

fn read_verifying_key(path: &Path) -> Result<VerifyingKey, CliError> {
    let bytes = std::fs::read(path).map_err(|e| CliError::Io {
        code: 2,
        msg: format!("read pubkey {}: {}", path.display(), e),
    })?;
    let raw = decode_key_bytes(&bytes, 32, "pubkey")?;
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&raw);
    VerifyingKey::from_bytes(arr).map_err(CliError::Evidence)
}

fn decode_key_bytes(bytes: &[u8], want_len: usize, label: &str) -> Result<Vec<u8>, CliError> {
    if bytes.len() == want_len {
        return Ok(bytes.to_vec());
    }
    let trimmed: String = std::str::from_utf8(bytes)
        .map_err(|_| {
            CliError::Usage(format!(
                "{} is not valid UTF-8 and not raw {}-byte blob",
                label, want_len
            ))
        })?
        .split_whitespace()
        .collect();
    if trimmed.len() == want_len * 2 {
        return hex::decode(&trimmed)
            .map_err(|e| CliError::Usage(format!("{} hex decode: {}", label, e)));
    }
    let decoded = decode_base64(&trimmed).ok_or_else(|| {
        CliError::Usage(format!(
            "{} encoding not recognised (raw|hex|base64)",
            label
        ))
    })?;
    if decoded.len() != want_len {
        return Err(CliError::Usage(format!(
            "{} decoded to {} bytes, want {}",
            label,
            decoded.len(),
            want_len
        )));
    }
    Ok(decoded)
}

/// Minimal base64 decoder (standard alphabet, padding required).
/// Hand-rolled to avoid pulling another runtime dep into the host
/// crate. Returns `None` if the input contains a non-alphabet byte.
fn decode_base64(s: &str) -> Option<Vec<u8>> {
    // `usize::is_multiple_of` is unstable on the pinned workspace
    // toolchain (nightly-2025-01-15) AND the lint name was added to
    // clippy only after that pin; silence both unknown_lints (older
    // pinned clippy) and the lint itself (newer +stable clippy) so
    // `scripts/fmt-clippy-deny.sh` and `just lint` both stay quiet.
    #[allow(unknown_lints, clippy::manual_is_multiple_of)]
    if s.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let mut group = [0u8; 4];
        let mut pad = 0;
        for j in 0..4 {
            let b = bytes[i + j];
            group[j] = match b {
                b'A'..=b'Z' => b - b'A',
                b'a'..=b'z' => b - b'a' + 26,
                b'0'..=b'9' => b - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' => {
                    pad += 1;
                    0
                }
                _ => return None,
            };
        }
        let chunk = (u32::from(group[0]) << 18)
            | (u32::from(group[1]) << 12)
            | (u32::from(group[2]) << 6)
            | u32::from(group[3]);
        out.push((chunk >> 16) as u8);
        if pad < 2 {
            out.push((chunk >> 8) as u8);
        }
        if pad < 1 {
            out.push(chunk as u8);
        }
        i += 4;
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn split_kv(arg: &str) -> Result<(&str, &str), CliError> {
    arg.split_once('=')
        .ok_or_else(|| CliError::Usage(format!("expected --key=value, got `{}`", arg)))
}

fn single_positional(sub: &str, rest: &[String]) -> Result<PathBuf, CliError> {
    if rest.len() != 1 {
        return Err(CliError::Usage(format!(
            "{} expects exactly one positional <bundle.tar.gz>",
            sub
        )));
    }
    Ok(PathBuf::from(&rest[0]))
}

/// Split `rest` into (positional, [(--key, value), ...]). Used by
/// `seal` and `verify` which both take exactly one positional path
/// followed by `--key=value` flags in any order.
fn positional_with_kv(
    sub: &str,
    rest: &[String],
) -> Result<(PathBuf, Vec<(String, String)>), CliError> {
    let mut path: Option<PathBuf> = None;
    let mut opts: Vec<(String, String)> = Vec::new();
    for arg in rest {
        if arg.starts_with("--") {
            let (k, v) = split_kv(arg)?;
            opts.push((k.to_string(), v.to_string()));
        } else if path.is_none() {
            path = Some(PathBuf::from(arg));
        } else {
            return Err(CliError::Usage(format!(
                "{} expects exactly one positional <bundle.tar.gz>",
                sub
            )));
        }
    }
    let path = path
        .ok_or_else(|| CliError::Usage(format!("{} expects a positional <bundle.tar.gz>", sub)))?;
    Ok((path, opts))
}

fn io_or_evidence(_sub: &'static str) -> impl Fn(EvidenceError) -> CliError {
    move |e| match &e {
        EvidenceError::MissingArtifact { .. } => CliError::Io {
            code: 2,
            msg: format!("{}", e),
        },
        _ => CliError::Evidence(e),
    }
}
