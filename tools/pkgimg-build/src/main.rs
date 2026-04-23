//! CONTEXT: Deterministic host builder for pkgimg v2 images from `.nxb` directories.
//! OWNERS: @runtime @storage
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Build sanity covered by `cargo test -p pkgimg-build`.

use std::fs;
use std::path::{Path, PathBuf};

use storage::pkgimg::{build_pkgimg, PkgImgCaps, PkgImgFileSpec};

fn usage() -> ! {
    eprintln!(
        "usage: pkgimg-build <output.pkgimg> <bundle@version.nxb-dir> [bundle@version.nxb-dir...]"
    );
    std::process::exit(2);
}

fn parse_bundle_and_version(name: &str) -> Option<(String, String)> {
    let stem = name.strip_suffix(".nxb").unwrap_or(name);
    let (bundle, version) = stem.rsplit_once('@')?;
    if bundle.is_empty() || version.is_empty() {
        return None;
    }
    Some((bundle.to_string(), version.to_string()))
}

fn collect_files(
    root: &Path,
    rel: &Path,
    out: &mut Vec<(PathBuf, Vec<u8>)>,
) -> Result<(), String> {
    let dir = root.join(rel);
    let entries = fs::read_dir(&dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("read_dir entry {}: {e}", dir.display()))?;
        let path = entry.path();
        let name = entry.file_name();
        let rel_path = rel.join(Path::new(&name));
        if path.is_dir() {
            collect_files(root, &rel_path, out)?;
            continue;
        }
        let bytes = fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        out.push((rel_path, bytes));
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        usage();
    }
    let output = PathBuf::from(&args[1]);
    let mut specs = Vec::new();

    for input in &args[2..] {
        let bundle_dir = PathBuf::from(input);
        let file_name = bundle_dir.file_name().and_then(|v| v.to_str()).unwrap_or_default();
        let (bundle, version) = parse_bundle_and_version(file_name).unwrap_or_else(|| {
            eprintln!(
                "invalid bundle dir '{}': expected <bundle>@<version>.nxb",
                bundle_dir.display()
            );
            std::process::exit(2);
        });
        let mut files = Vec::new();
        collect_files(&bundle_dir, Path::new(""), &mut files).unwrap_or_else(|err| {
            eprintln!("{err}");
            std::process::exit(1);
        });
        files.sort_by(|a, b| a.0.cmp(&b.0));
        for (rel, bytes) in files {
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            specs.push(PkgImgFileSpec::new(&bundle, &version, &rel_str, &bytes));
        }
    }

    let image = build_pkgimg(&specs, PkgImgCaps::default()).unwrap_or_else(|err| {
        eprintln!("pkgimg build failed: {err}");
        std::process::exit(1);
    });
    fs::write(&output, image).unwrap_or_else(|err| {
        eprintln!("write {}: {err}", output.display());
        std::process::exit(1);
    });
}
