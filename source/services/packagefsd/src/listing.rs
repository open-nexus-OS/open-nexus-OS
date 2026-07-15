// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Pure directory-listing logic shared by BOTH packagefsd servers
//! (os_lite raw path and std capnp path) so listing semantics cannot drift:
//! canonical byte-order of names, deduplicated implicit directories, RFC-0072
//! error mapping. No IPC, no registry types — callers feed iterators.
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0291)
//! TEST_COVERAGE: unit tests below (roots, nesting, not-found, not-dir)

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use nexus_vfs_types::{DirEntry, FileKind, VfsError};

/// The `rel` value that addresses the bundle-roots listing.
pub const ROOT_REL: &str = ".";

const KIND_DIRECTORY: u16 = 1;

/// Lists the bundle roots (active bundle names) as directories, in canonical
/// byte order.
pub fn list_roots<'a>(names: impl Iterator<Item = &'a str>) -> Vec<DirEntry> {
    let sorted: BTreeMap<&str, ()> = names.map(|n| (n, ())).collect();
    sorted
        .keys()
        .map(|name| DirEntry { name: (*name).to_string(), kind: FileKind::Dir, size: 0 })
        .collect()
}

/// Lists the direct children of `sub` within one bundle's entry set.
///
/// `paths` yields every stored entry as `(path, kind, size)`; `sub = ""`
/// addresses the bundle root. Intermediate directories that exist only as
/// path prefixes are synthesized as `Dir` entries. Fail-closed mapping:
/// unknown `sub` → `NotFound`; `sub` names a file → `NotDir`.
pub fn list_children<'a>(
    paths: impl Iterator<Item = (&'a str, u16, u64)>,
    sub: &str,
) -> Result<Vec<DirEntry>, VfsError> {
    let mut children: BTreeMap<String, (FileKind, u64)> = BTreeMap::new();
    let mut sub_seen_as_dir = sub.is_empty();
    let mut sub_seen_as_file = false;

    for (path, kind, size) in paths {
        if path == ROOT_REL {
            continue;
        }
        if path == sub {
            if kind == KIND_DIRECTORY {
                sub_seen_as_dir = true;
            } else {
                sub_seen_as_file = true;
            }
            continue;
        }
        let rest = if sub.is_empty() {
            path
        } else {
            match path.strip_prefix(sub).and_then(|r| r.strip_prefix('/')) {
                Some(rest) => {
                    sub_seen_as_dir = true;
                    rest
                }
                None => continue,
            }
        };
        if rest.is_empty() {
            continue;
        }
        match rest.split_once('/') {
            // Deeper path: the first segment is an (implicit) directory.
            Some((first, _)) => {
                children.entry(first.to_string()).or_insert((FileKind::Dir, 0));
            }
            None => {
                let child_kind =
                    if kind == KIND_DIRECTORY { FileKind::Dir } else { FileKind::File };
                let entry = children.entry(rest.to_string()).or_insert((child_kind, size));
                // An explicit dir entry never downgrades a synthesized one.
                if child_kind == FileKind::Dir {
                    entry.0 = FileKind::Dir;
                    entry.1 = 0;
                }
            }
        }
    }

    if sub_seen_as_file {
        return Err(VfsError::NotDir);
    }
    if !sub_seen_as_dir {
        return Err(VfsError::NotFound);
    }
    Ok(children
        .into_iter()
        .map(|(name, (kind, size))| DirEntry { name, kind, size })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries() -> Vec<(&'static str, u16, u64)> {
        // Mirrors a real bundle: "." root marker, files, one nested path with
        // no explicit dir entry for "assets".
        alloc::vec![
            (".", KIND_DIRECTORY, 0),
            ("manifest.nxb", 0, 42),
            ("payload.elf", 0, 1024),
            ("assets/icon.svg", 0, 512),
            ("assets/deep/tex.png", 0, 99),
        ]
    }

    #[test]
    fn roots_are_sorted_dirs() {
        let roots = list_roots(["system", "chat", "demo.hello"].into_iter());
        let names: Vec<&str> = roots.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["chat", "demo.hello", "system"]);
        assert!(roots.iter().all(|e| e.kind == FileKind::Dir && e.size == 0));
    }

    #[test]
    fn bundle_root_children_synthesize_dirs() {
        let listing = list_children(entries().into_iter(), "").expect("root listing");
        let got: Vec<(&str, FileKind, u64)> =
            listing.iter().map(|e| (e.name.as_str(), e.kind, e.size)).collect();
        assert_eq!(
            got,
            [
                ("assets", FileKind::Dir, 0),
                ("manifest.nxb", FileKind::File, 42),
                ("payload.elf", FileKind::File, 1024),
            ]
        );
    }

    #[test]
    fn nested_listing_descends() {
        let listing = list_children(entries().into_iter(), "assets").expect("assets listing");
        let got: Vec<(&str, FileKind)> =
            listing.iter().map(|e| (e.name.as_str(), e.kind)).collect();
        assert_eq!(got, [("deep", FileKind::Dir), ("icon.svg", FileKind::File)]);
    }

    #[test]
    fn test_reject_unknown_subdir_not_found() {
        assert_eq!(list_children(entries().into_iter(), "nope"), Err(VfsError::NotFound));
    }

    #[test]
    fn test_reject_file_as_dir() {
        assert_eq!(
            list_children(entries().into_iter(), "payload.elf"),
            Err(VfsError::NotDir)
        );
    }
}
