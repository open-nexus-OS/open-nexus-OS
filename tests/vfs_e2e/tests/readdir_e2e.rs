// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: End-to-end host proof for VFS ReadDir (RFC-0072 Phase 1 /
//! TASK-0291): VfsClient → vfsd → packagefsd over loopback, bounded
//! pagination, canonical order, stable error codes.
//! OWNERS: @runtime
//! STATUS: Functional
//! TEST_COVERAGE: roots listing, bundle listing, pagination, negative codes
#![cfg(nexus_env = "host")]

use std::sync::Arc;
use std::thread;

use nexus_packagefs::PackageFsClient;
use nexus_vfs::VfsClient;
use nexus_vfs_types::{FileKind, VfsError};
use packagefsd::FileEntry;

const KIND_FILE: u16 = 0;

fn spawn_stack() -> (VfsClient, thread::JoinHandle<()>, thread::JoinHandle<()>) {
    let (pkg_client, mut pkg_server) = packagefsd::loopback_transport();
    let registry = packagefsd::BundleRegistry::default();
    registry
        .publish_bundle(
            "demo.hello",
            "1.0.0",
            vec![
                FileEntry::new("manifest.nxb", KIND_FILE, b"nxb".to_vec()),
                FileEntry::new("payload.elf", KIND_FILE, b"elf-bytes".to_vec()),
                FileEntry::new("assets/logo.svg", KIND_FILE, b"<svg/>".to_vec()),
            ],
        )
        .expect("publish demo.hello");
    registry
        .publish_bundle(
            "system",
            "1.0.0",
            vec![FileEntry::new("build.prop", KIND_FILE, b"ro.nexus.build=dev\n".to_vec())],
        )
        .expect("publish system");

    let pkg_thread = thread::spawn(move || {
        packagefsd::run_with_transport(&mut pkg_server, registry).unwrap();
    });
    let packagefs_client = Arc::new(PackageFsClient::from_loopback(pkg_client));

    let (vfs_conn, mut vfs_server) = vfsd::loopback_transport();
    let vfs_thread = thread::spawn(move || {
        vfsd::run_with_transport(&mut vfs_server, packagefs_client).unwrap();
    });
    (VfsClient::from_loopback(vfs_conn), pkg_thread, vfs_thread)
}

#[test]
fn readdir_end_to_end() {
    let (client, _pkg_thread, _vfs_thread) = spawn_stack();

    // Namespace root lists the active bundles as directories, sorted.
    let roots = client.read_dir("pkg:/", 0, 64).expect("root listing");
    let names: Vec<&str> = roots.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["demo.hello", "system"]);
    assert!(roots.entries.iter().all(|e| e.kind == FileKind::Dir));
    assert!(roots.eof);

    // Bundle listing: synthesized "assets" dir + files with real sizes.
    let bundle = client.read_dir("pkg:/demo.hello", 0, 64).expect("bundle listing");
    let got: Vec<(&str, FileKind, u64)> =
        bundle.entries.iter().map(|e| (e.name.as_str(), e.kind, e.size)).collect();
    assert_eq!(
        got,
        [
            ("assets", FileKind::Dir, 0),
            ("manifest.nxb", FileKind::File, 3),
            ("payload.elf", FileKind::File, 9),
        ]
    );

    // Nested dir.
    let assets = client.read_dir("pkg:/demo.hello/assets", 0, 64).expect("assets listing");
    assert_eq!(assets.entries.len(), 1);
    assert_eq!(assets.entries[0].name, "logo.svg");

    // Pagination: limit 1 walks the same set deterministically.
    let mut paged = Vec::new();
    let mut cursor = 0u32;
    loop {
        let page = client.read_dir("pkg:/demo.hello", cursor, 1).expect("page");
        assert!(page.entries.len() <= 1);
        paged.extend(page.entries);
        cursor = page.next_cursor;
        if page.eof {
            break;
        }
    }
    assert_eq!(paged, bundle.entries);
}

#[test]
fn test_reject_readdir_error_codes() {
    let (client, _pkg_thread, _vfs_thread) = spawn_stack();

    // Unknown bundle → stable ENOTFOUND from the provider.
    match client.read_dir("pkg:/nope", 0, 64) {
        Err(nexus_vfs::Error::Vfs(VfsError::NotFound)) => {}
        other => panic!("expected ENOTFOUND, got {other:?}"),
    }
    // Listing a file → ENOTDIR.
    match client.read_dir("pkg:/demo.hello/payload.elf", 0, 64) {
        Err(nexus_vfs::Error::Vfs(VfsError::NotDir)) => {}
        other => panic!("expected ENOTDIR, got {other:?}"),
    }
    // Traversal is rejected client-side before any IPC.
    match client.read_dir("pkg:/demo.hello/../secret", 0, 64) {
        Err(nexus_vfs::Error::InvalidPath) => {}
        other => panic!("expected InvalidPath, got {other:?}"),
    }
}
