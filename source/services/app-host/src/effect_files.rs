// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: the `svc.files` surface of the app-host DSL `EffectHost`
//! (RFC-0073) — listing, counting, the write ops (mkdir/remove/rename/copy),
//! `stat`, and the record/format helpers the rows are built from. Split out of
//! `effect_host.rs` (structure-gate) when the RFC-0084 Phase 6 filter pushed
//! that file past its ratchet.
//!
//! `list` and `count` share ONE visibility predicate
//! ([`crate::file_filter::name_visible`], host-tested) so the object counter
//! can never disagree with the rows on screen.
//!
//! OWNERS: @ui @runtime
//! STATUS: Functional (RFC-0073, filter RFC-0084 Phase 6)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: filter predicate unit-tested on the host; the transport is
//! proven via QEMU markers (`apphost: dsl svc files.*`)

#![cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]

use super::effect_host::{
    call_reply, raw_marker, AppEffectHost, ERR_SVC_SHAPE, ERR_SVC_UNAVAILABLE, ERR_SVC_UNKNOWN,
    FILES_REPLY_BUF, REPLY_BUF, VFS_OPCODE_READDIR, VFS_OPCODE_STAT,
};
use alloc::string::String;
use alloc::vec::Vec;
use nexus_dsl_runtime::Value;

impl AppEffectHost {
    /// Builds one `FileEntry` record (field-sorted, `Value::Record` contract).
    /// `icon` is a `"mime:<stem>"` source (RFC-0073 / TASK-0294): directories
    /// resolve to the directory stem, files to their extension's icon stem via
    /// the mime SSOT. The DSL `Image` primitive turns it into a baked sprite.
    fn file_entry_record(&self, name_sym: u32, entry: &nexus_vfs_types::DirEntry) -> Value {
        let mut fields = alloc::vec![(name_sym, Value::Str(entry.name.clone()))];
        if let Some(sym) = self.id_sym {
            fields.push((sym, Value::Str(entry.name.clone())));
        }
        if let Some(sym) = self.kind_sym {
            fields.push((sym, Value::Str(entry.kind.label().into())));
        }
        if let Some(sym) = self.size_sym {
            fields.push((sym, Value::Int(entry.size.min(i64::MAX as u64) as i64)));
        }
        if let Some(sym) = self.size_text_sym {
            fields.push((sym, Value::Str(format_size(entry.size, entry.kind))));
        }
        if let Some(sym) = self.icon_sym {
            let stem = entry_icon_stem(entry);
            fields.push((sym, Value::Str(alloc::format!("mime:{stem}"))));
        }
        if let Some(sym) = self.date_sym {
            fields.push((sym, Value::Str(stub_date(&entry.name, entry.kind))));
        }
        fields.sort_by_key(|(sym, _)| *sym);
        Value::Record(fields)
    }

    /// One bounded ReadDir page from vfsd (shared by list/count/recent).
    fn readdir_page(&self, path: &str, cursor: u32) -> Result<nexus_vfs_types::ReadDirPage, u32> {
        let send_slot = Self::svc_send_slot("files").ok_or(ERR_SVC_UNKNOWN)?;
        let payload =
            nexus_vfs_types::encode_readdir_request(path, cursor, 64).map_err(|_| ERR_SVC_SHAPE)?;
        let mut req = Vec::with_capacity(1 + payload.len());
        req.push(VFS_OPCODE_READDIR);
        req.extend_from_slice(&payload);
        let mut resp = alloc::vec![0u8; FILES_REPLY_BUF];
        let Some(len) = call_reply(send_slot, &req, &mut resp) else {
            raw_marker("apphost: dsl svc files readdir FAIL (vfsd unreachable)");
            return Err(ERR_SVC_UNAVAILABLE);
        };
        nexus_vfs_types::decode_readdir_response(&resp[..len]).map_err(|err| {
            let mut line = String::from("apphost: dsl svc files.list deny (");
            line.push_str(err.name());
            line.push(')');
            raw_marker(&line);
            100 + u32::from(err.code())
        })
    }

    /// The "Aktuelle Dateien" aggregation: every FILE across the home's
    /// top-level folders (Papierkorb excluded) plus root files, names prefixed
    /// with their folder ("Bilder/IMG.jpg"), newest (stub date) first. Bounded:
    /// one page per folder, 64 entries total.
    fn collect_recent(&self) -> Result<Vec<nexus_vfs_types::DirEntry>, u32> {
        let root = self.readdir_page("/", 0)?;
        let mut out: Vec<nexus_vfs_types::DirEntry> = Vec::new();
        for entry in &root.entries {
            match entry.kind {
                nexus_vfs_types::FileKind::Dir if entry.name != "Papierkorb" => {
                    let mut sub_path = String::from("/");
                    sub_path.push_str(&entry.name);
                    if let Ok(sub) = self.readdir_page(&sub_path, 0) {
                        for file in sub.entries {
                            if file.kind == nexus_vfs_types::FileKind::File {
                                let mut name = entry.name.clone();
                                name.push('/');
                                name.push_str(&file.name);
                                out.push(nexus_vfs_types::DirEntry {
                                    name,
                                    kind: file.kind,
                                    size: file.size,
                                });
                            }
                        }
                    }
                }
                nexus_vfs_types::FileKind::File => out.push(entry.clone()),
                _ => {}
            }
        }
        // Newest first (stub date over the basename), ties by name.
        out.sort_by(|a, b| {
            stub_date_key(&b.name, b.kind)
                .cmp(&stub_date_key(&a.name, a.kind))
                .then_with(|| a.name.cmp(&b.name))
        });
        out.truncate(64);
        Ok(out)
    }

    /// `svc.files.list(path, cursor)` → `List<FileEntry{ name, kind, size }>`
    /// — one bounded ReadDir page from vfsd (RFC-0072/0073; `cursor` 0 = first
    /// page, continuation via the page's next cursor). The pseudo-path
    /// `"recent:"` aggregates files across the home folders, newest first.
    /// Thin adapter over the host-tested predicate in [`crate::file_filter`].
    fn entry_visible(entry: &nexus_vfs_types::DirEntry, query: &str, show_hidden: bool) -> bool {
        crate::file_filter::name_visible(entry.name.as_str(), query, show_hidden)
    }

    pub(crate) fn files_list(
        &self,
        path: &str,
        cursor: i64,
        sort: &str,
        query: &str,
        show_hidden: bool,
    ) -> Result<Value, u32> {
        let Some(name_sym) = self.name_sym else {
            raw_marker("apphost: dsl svc files.list FAIL (no name symbol)");
            return Err(ERR_SVC_SHAPE);
        };
        if path == "recent:" {
            let entries = self.collect_recent()?;
            let rows: Vec<Value> = entries
                .iter()
                .filter(|entry| Self::entry_visible(entry, query, show_hidden))
                .map(|entry| self.file_entry_record(name_sym, entry))
                .collect();
            let mut line = String::from("apphost: dsl svc files.list ok (n=");
            let _ = core::fmt::write(&mut line, format_args!("{}, recent)", rows.len()));
            raw_marker(&line);
            return Ok(Value::List(rows));
        }
        let cursor = u32::try_from(cursor).map_err(|_| ERR_SVC_SHAPE)?;
        let page = self.readdir_page(path, cursor)?;
        // Sort the page (name | kind | date). Directories always sort
        // before files; ties break by name. "date" uses the stub key.
        let dir_first =
            |e: &nexus_vfs_types::DirEntry| u8::from(e.kind != nexus_vfs_types::FileKind::Dir);
        let mut entries: Vec<&nexus_vfs_types::DirEntry> = page
            .entries
            .iter()
            .filter(|entry| Self::entry_visible(entry, query, show_hidden))
            .collect();
        match sort {
            "kind" => entries.sort_by(|a, b| {
                dir_first(a)
                    .cmp(&dir_first(b))
                    .then_with(|| entry_icon_stem(a).cmp(entry_icon_stem(b)))
                    .then_with(|| a.name.cmp(&b.name))
            }),
            "date" => entries.sort_by(|a, b| {
                dir_first(a)
                    .cmp(&dir_first(b))
                    .then_with(|| {
                        stub_date_key(&a.name, a.kind).cmp(&stub_date_key(&b.name, b.kind))
                    })
                    .then_with(|| a.name.cmp(&b.name))
            }),
            _ => entries
                .sort_by(|a, b| dir_first(a).cmp(&dir_first(b)).then_with(|| a.name.cmp(&b.name))),
        }
        let rows: Vec<Value> =
            entries.iter().map(|entry| self.file_entry_record(name_sym, entry)).collect();
        let mut line = String::from("apphost: dsl svc files.list ok (n=");
        let _ = core::fmt::write(&mut line, format_args!("{})", rows.len()));
        raw_marker(&line);
        // Count entries whose type resolved to real artwork (not the
        // octet-stream fallback) — the file-type icon pipeline proof.
        let resolved = page
            .entries
            .iter()
            .filter(|entry| entry_icon_stem(entry) != nexus_mime_icons::UNKNOWN)
            .count();
        let mut icons = String::from("stash: mime icons resolved (n=");
        let _ = core::fmt::write(&mut icons, format_args!("{})", resolved));
        raw_marker(&icons);
        Ok(Value::List(rows))
    }

    /// `svc.files.count(path)` → `Int` — the entry count of a folder (or the
    /// recent aggregation). Drives the honest empty-folder state in the UI.
    pub(crate) fn files_count(
        &self,
        path: &str,
        query: &str,
        show_hidden: bool,
    ) -> Result<Value, u32> {
        // Same predicate as `files_list` — see `entry_visible`.
        let n = if path == "recent:" {
            self.collect_recent()?
                .iter()
                .filter(|entry| Self::entry_visible(entry, query, show_hidden))
                .count()
        } else {
            self.readdir_page(path, 0)?
                .entries
                .iter()
                .filter(|entry| Self::entry_visible(entry, query, show_hidden))
                .count()
        };
        Ok(Value::Int(n as i64))
    }

    /// `svc.files.mkdir(path)` → `Bool` (RFC-0073 Phase 2 write surface).
    /// Routed to vfsd, which forwards to the nxfs `/data` store.
    pub(crate) fn files_write(&self, opcode: u8, path: &str, marker: &str) -> Result<Value, u32> {
        let send_slot = Self::svc_send_slot("files").ok_or(ERR_SVC_UNKNOWN)?;
        let payload = nexus_vfs_types::fileops::encode_path_request(path).ok_or(ERR_SVC_SHAPE)?;
        let mut req = Vec::with_capacity(1 + payload.len());
        req.push(opcode);
        req.extend_from_slice(&payload);
        let mut resp = [0u8; 16];
        let Some(len) = call_reply(send_slot, &req, &mut resp) else {
            raw_marker("apphost: dsl svc files write FAIL (vfsd unreachable)");
            return Err(ERR_SVC_UNAVAILABLE);
        };
        match nexus_vfs_types::fileops::decode_status_reply(&resp[..len]) {
            Some(code) if code == nexus_vfs_types::CODE_OK => {
                raw_marker(marker);
                Ok(Value::Bool(true))
            }
            Some(code) => {
                let mut line = String::from("apphost: dsl svc files write deny (");
                if let Some(err) = nexus_vfs_types::VfsError::from_code(code) {
                    line.push_str(err.name());
                }
                line.push(')');
                raw_marker(&line);
                Ok(Value::Bool(false))
            }
            None => Err(ERR_SVC_SHAPE),
        }
    }

    /// `svc.files.rename(from, to)` → `Bool` (RFC-0073). Powers both in-place
    /// rename and MOVE (a rename across directories); nxfs `Op::Rename` handles
    /// the cross-directory case.
    pub(crate) fn files_rename(&self, from: &str, to: &str) -> Result<Value, u32> {
        let send_slot = Self::svc_send_slot("files").ok_or(ERR_SVC_UNKNOWN)?;
        let payload = nexus_vfs_types::fileops::encode_rename(from, to).ok_or(ERR_SVC_SHAPE)?;
        let mut req = Vec::with_capacity(1 + payload.len());
        req.push(nexus_vfs_types::fileops::OP_RENAME);
        req.extend_from_slice(&payload);
        let mut resp = [0u8; 16];
        let Some(len) = call_reply(send_slot, &req, &mut resp) else {
            raw_marker("apphost: dsl svc files.rename FAIL (vfsd unreachable)");
            return Err(ERR_SVC_UNAVAILABLE);
        };
        match nexus_vfs_types::fileops::decode_status_reply(&resp[..len]) {
            Some(code) if code == nexus_vfs_types::CODE_OK => {
                raw_marker("apphost: dsl svc files.rename ok");
                Ok(Value::Bool(true))
            }
            Some(code) => {
                let mut line = String::from("apphost: dsl svc files.rename deny (");
                if let Some(err) = nexus_vfs_types::VfsError::from_code(code) {
                    line.push_str(err.name());
                }
                line.push(')');
                raw_marker(&line);
                Ok(Value::Bool(false))
            }
            None => Err(ERR_SVC_SHAPE),
        }
    }

    /// `svc.files.copy(from, to)` → `Bool` — copy a file (nxfs read + create +
    /// write behind OP_COPY; a directory source fails honestly).
    pub(crate) fn files_copy(&self, from: &str, to: &str) -> Result<Value, u32> {
        let send_slot = Self::svc_send_slot("files").ok_or(ERR_SVC_UNKNOWN)?;
        let payload = nexus_vfs_types::fileops::encode_rename(from, to).ok_or(ERR_SVC_SHAPE)?;
        let mut req = Vec::with_capacity(1 + payload.len());
        req.push(nexus_vfs_types::fileops::OP_COPY);
        req.extend_from_slice(&payload);
        let mut resp = [0u8; 16];
        let Some(len) = call_reply(send_slot, &req, &mut resp) else {
            raw_marker("apphost: dsl svc files.copy FAIL (vfsd unreachable)");
            return Err(ERR_SVC_UNAVAILABLE);
        };
        match nexus_vfs_types::fileops::decode_status_reply(&resp[..len]) {
            Some(code) if code == nexus_vfs_types::CODE_OK => {
                raw_marker("apphost: dsl svc files.copy ok");
                Ok(Value::Bool(true))
            }
            Some(code) => {
                let mut line = String::from("apphost: dsl svc files.copy deny (");
                if let Some(err) = nexus_vfs_types::VfsError::from_code(code) {
                    line.push_str(err.name());
                }
                line.push(')');
                raw_marker(&line);
                Ok(Value::Bool(false))
            }
            None => Err(ERR_SVC_SHAPE),
        }
    }

    /// `svc.files.stat(path)` → `FileEntry` for a single path.
    pub(crate) fn files_stat(&self, path: &str) -> Result<Value, u32> {
        let Some(name_sym) = self.name_sym else {
            raw_marker("apphost: dsl svc files.stat FAIL (no name symbol)");
            return Err(ERR_SVC_SHAPE);
        };
        let send_slot = Self::svc_send_slot("files").ok_or(ERR_SVC_UNKNOWN)?;
        let mut req = Vec::with_capacity(1 + path.len());
        req.push(VFS_OPCODE_STAT);
        req.extend_from_slice(path.as_bytes());
        let mut resp = [0u8; REPLY_BUF];
        let Some(len) = call_reply(send_slot, &req, &mut resp) else {
            raw_marker("apphost: dsl svc files.stat FAIL (vfsd unreachable)");
            return Err(ERR_SVC_UNAVAILABLE);
        };
        // vfsd bring-up stat reply: [1, size u64 LE, kind u16 LE] | [0].
        let frame = &resp[..len];
        if frame.len() < 1 + 8 + 2 || frame[0] != 1 {
            raw_marker("apphost: dsl svc files.stat deny (ENOTFOUND)");
            return Err(100 + u32::from(nexus_vfs_types::VfsError::NotFound.code()));
        }
        let size = u64::from_le_bytes([
            frame[1], frame[2], frame[3], frame[4], frame[5], frame[6], frame[7], frame[8],
        ]);
        let kind = match u16::from_le_bytes([frame[9], frame[10]]) {
            1 => nexus_vfs_types::FileKind::Dir,
            _ => nexus_vfs_types::FileKind::File,
        };
        let name = path.rsplit('/').next().unwrap_or(path);
        let entry = nexus_vfs_types::DirEntry { name: String::from(name), kind, size };
        raw_marker("apphost: dsl svc files.stat ok");
        Ok(self.file_entry_record(name_sym, &entry))
    }
}

/// A deterministic STUB modified-date for a listing entry. The OS has no
/// real-time clock yet, so timestamps are demo values derived from the name —
/// stable per file (so re-listing shows the same date) and varied enough that
/// date-sort is meaningful. Directories carry no date ("-"), matching the
/// design. Real per-file stored timestamps land once an RTC service exists.
fn stub_date(name: &str, kind: nexus_vfs_types::FileKind) -> String {
    if kind == nexus_vfs_types::FileKind::Dir {
        return String::from("-");
    }
    let (year, month, day) = stub_ymd(name);
    let mut out = String::new();
    push_two(&mut out, day);
    out.push('.');
    push_two(&mut out, month);
    out.push('.');
    let _ = core::fmt::write(&mut out, format_args!("{year}"));
    out
}

/// FNV-1a of the BASENAME → a demo `(year, month, day)` in mid-2026. Basename
/// so "Bilder/IMG.jpg" (the recent view's prefixed form) dates identically to
/// "IMG.jpg" in its folder view.
fn stub_ymd(name: &str) -> (u32, u32, u32) {
    let base = name.rsplit('/').next().unwrap_or(name);
    let mut hash: u32 = 2166136261;
    for byte in base.bytes() {
        hash = (hash ^ u32::from(byte)).wrapping_mul(16777619);
    }
    let day = 1 + (hash % 28);
    let month = 5 + ((hash / 28) % 3);
    (2026, month, day)
}

/// A sortable key for the stub date (directories sort first with key 0).
fn stub_date_key(name: &str, kind: nexus_vfs_types::FileKind) -> u32 {
    if kind == nexus_vfs_types::FileKind::Dir {
        return 0;
    }
    let (year, month, day) = stub_ymd(name);
    year * 10000 + month * 100 + day
}

/// Appends a zero-padded two-digit number.
fn push_two(out: &mut String, value: u32) {
    if value < 10 {
        out.push('0');
    }
    let _ = core::fmt::write(out, format_args!("{value}"));
}

/// The mime icon stem for a listing entry (RFC-0073): directories use the
/// directory stem, files resolve by extension through the mime SSOT.
fn entry_icon_stem(entry: &nexus_vfs_types::DirEntry) -> &'static str {
    match entry.kind {
        nexus_vfs_types::FileKind::Dir => nexus_mime_icons::DIRECTORY,
        nexus_vfs_types::FileKind::File => nexus_mime_icons::stem_for_file_name(&entry.name),
    }
}

/// Human-readable size for direct UI binding (`12 B` / `4.2 KB` / `3.8 MB`);
/// directories render as a plain dash (ASCII — the baked UI font has no
/// em-dash glyph; it renders as `?`). Integer math only (no_std, no floats).
fn format_size(size: u64, kind: nexus_vfs_types::FileKind) -> String {
    if kind == nexus_vfs_types::FileKind::Dir {
        return String::from("-");
    }
    let mut out = String::new();
    let (scaled_x10, unit) = if size >= 1024 * 1024 * 1024 {
        (size * 10 / (1024 * 1024 * 1024), "GB")
    } else if size >= 1024 * 1024 {
        (size * 10 / (1024 * 1024), "MB")
    } else if size >= 1024 {
        (size * 10 / 1024, "KB")
    } else {
        let _ = core::fmt::write(&mut out, format_args!("{size} B"));
        return out;
    };
    let whole = scaled_x10 / 10;
    let tenth = scaled_x10 % 10;
    if tenth == 0 {
        let _ = core::fmt::write(&mut out, format_args!("{whole} {unit}"));
    } else {
        let _ = core::fmt::write(&mut out, format_args!("{whole}.{tenth} {unit}"));
    }
    out
}
