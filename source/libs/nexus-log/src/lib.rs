// Copyright 2025 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unified, deterministic logging facade shared by kernel and userspace.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests
//! ADR: docs/rfcs/RFC-0003-unified-logging.md
//!
//! This is the first step toward RFC-0003 (unified logging). For now it only
//! offers raw line emission with compile-time level gating and a per-domain
//! prefix. Future work (tracked in the RFC) will layer richer routing,
//! formatting, and runtime configuration on top.

#![no_std]

#[cfg(all(feature = "sink-logd", target_arch = "riscv64", target_os = "none"))]
extern crate alloc;

use core::fmt;
use core::ops::{BitOr, BitOrAssign};
use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};

#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
use core::sync::atomic::AtomicBool;

#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
use core::sync::atomic::AtomicUsize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Level {
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    Trace = 5,
}

impl Level {
    fn label(self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        }
    }
}

static MAX_LEVEL: AtomicU8 = AtomicU8::new(Level::Debug as u8);
static TOPIC_MASK: AtomicU32 = AtomicU32::new(u32::MAX);
const MAX_SLICE_LEN: usize = 0x4000;

pub fn set_max_level(level: Level) {
    MAX_LEVEL.store(level as u8, Ordering::Relaxed);
}

pub fn set_topic_mask(mask: Topic) {
    TOPIC_MASK.store(mask.bits(), Ordering::Relaxed);
}

fn level_enabled(level: Level) -> bool {
    level as u8 <= MAX_LEVEL.load(Ordering::Relaxed)
}

fn topic_enabled(topic: Topic) -> bool {
    let mask = TOPIC_MASK.load(Ordering::Relaxed);
    let bits = topic.bits();
    if bits == 0 {
        return true;
    }
    (mask & bits) == bits
}

pub fn error(target: &str, f: impl FnOnce(&mut LineBuilder)) {
    log(LineMeta { level: Level::Error, target, topic: TOPIC_GENERAL }, f);
}

pub fn error_topic(target: &str, topic: Topic, f: impl FnOnce(&mut LineBuilder)) {
    log(LineMeta { level: Level::Error, target, topic }, f);
}

pub fn warn(target: &str, f: impl FnOnce(&mut LineBuilder)) {
    log(LineMeta { level: Level::Warn, target, topic: TOPIC_GENERAL }, f);
}

pub fn warn_topic(target: &str, topic: Topic, f: impl FnOnce(&mut LineBuilder)) {
    log(LineMeta { level: Level::Warn, target, topic }, f);
}

pub fn info(target: &str, f: impl FnOnce(&mut LineBuilder)) {
    log(LineMeta { level: Level::Info, target, topic: TOPIC_GENERAL }, f);
}

pub fn info_topic(target: &str, topic: Topic, f: impl FnOnce(&mut LineBuilder)) {
    log(LineMeta { level: Level::Info, target, topic }, f);
}

pub fn debug(target: &str, f: impl FnOnce(&mut LineBuilder)) {
    log(LineMeta { level: Level::Debug, target, topic: TOPIC_GENERAL }, f);
}

pub fn debug_topic(target: &str, topic: Topic, f: impl FnOnce(&mut LineBuilder)) {
    log(LineMeta { level: Level::Debug, target, topic }, f);
}

pub fn trace(target: &str, f: impl FnOnce(&mut LineBuilder)) {
    log(LineMeta { level: Level::Trace, target, topic: TOPIC_GENERAL }, f);
}

pub fn trace_topic(target: &str, topic: Topic, f: impl FnOnce(&mut LineBuilder)) {
    log(LineMeta { level: Level::Trace, target, topic }, f);
}

pub fn info_static(target: &str, message: &str) {
    info(target, |line| line.text_ref(StrRef::from(message)));
}

pub fn warn_static(target: &str, message: &str) {
    warn(target, |line| line.text_ref(StrRef::from(message)));
}

pub fn debug_static(target: &str, message: &str) {
    debug(target, |line| line.text_ref(StrRef::from(message)));
}

pub fn log(meta: LineMeta<'_>, f: impl FnOnce(&mut LineBuilder)) {
    if !level_enabled(meta.level) || !topic_enabled(meta.topic) {
        return;
    }

    #[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
    {
        debug_ptr(b'L', meta.level.label().as_ptr() as usize);
        debug_ptr(b'T', meta.target.as_ptr() as usize);
    }

    let mut sink = sink::Sink::new(meta.level, meta.target, meta.topic);
    sink.write_byte(b'[');
    sink.write_str(meta.level.label());
    sink.write_byte(b' ');
    sink.write_str(meta.target);
    sink.write_byte(b']');
    sink.write_byte(b' ');

    {
        let mut builder = LineBuilder { sink: &mut sink };
        f(&mut builder);
    }

    sink.write_byte(b'\n');

    #[cfg(all(feature = "sink-logd", target_arch = "riscv64", target_os = "none"))]
    {
        sink_logd::try_append(meta.level, meta.target, sink.capture_bytes());
    }
}

pub struct LineMeta<'a> {
    pub level: Level,
    pub target: &'a str,
    pub topic: Topic,
}

#[derive(Clone, Copy)]
pub struct StrRef<'a> {
    ptr: usize,
    len: usize,
    value: Option<&'a str>,
}

impl<'a> StrRef<'a> {
    pub fn new(value: &'a str) -> Self {
        let ptr = value.as_ptr() as usize;
        let len = value.len();
        let value = if str_ref_permitted(ptr, len) { Some(value) } else { None };
        Self { ptr, len, value }
    }

    pub fn from_unchecked(value: &'a str) -> Self {
        Self { ptr: value.as_ptr() as usize, len: value.len(), value: Some(value) }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn ptr(&self) -> usize {
        self.ptr
    }

    fn bytes(&self) -> Option<&'a [u8]> {
        self.value.map(|val| val.as_bytes())
    }
}

impl<'a> From<&'a str> for StrRef<'a> {
    fn from(value: &'a str) -> Self {
        Self::new(value)
    }
}

fn str_ref_permitted(ptr: usize, len: usize) -> bool {
    if len == 0 {
        return true;
    }
    if len > MAX_SLICE_LEN {
        return false;
    }

    #[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
    {
        if !guard_logs_enabled() {
            return true;
        }
        if !is_sv39_canonical(ptr) {
            return false;
        }
        let end = match ptr.checked_add(len) {
            Some(val) => val,
            None => return false,
        };
        if USERS_BOUNDS_ONCE
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            if guard_logs_enabled() {
                let (start, limit) = image_bounds();
                debug_ptr(b'S', start);
                debug_ptr(b'E', limit);
            }
        }
        if guard_logs_enabled() && USERS_STR_DIAG.fetch_add(1, Ordering::Relaxed) < PROBE_LIMIT {
            debug_ptr(b'@', ptr);
            debug_ptr(b'%', end);
            debug_ptr(b'&', len);
        }
        let (start, limit) = image_bounds();
        let in_range = ptr >= start && end <= limit;
        if in_range {
            if USERS_STR_GOOD.fetch_add(1, Ordering::Relaxed) < PROBE_LIMIT {
                trace_good_str(ptr, len);
            }
            return true;
        }
        if USERS_BAD_PTRS.fetch_add(1, Ordering::Relaxed) < PROBE_LIMIT {
            debug_ptr(b'P', ptr);
            debug_ptr(b'Q', end);
            debug_ptr(b'L', len);
        }
        false
    }
    #[cfg(not(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (ptr, len);
        true
    }
}

pub struct LineBuilder<'a, 'meta> {
    sink: &'a mut sink::Sink<'meta>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Topic(u32);

impl Topic {
    pub const fn empty() -> Self {
        Topic(0)
    }

    pub const fn bit(bit: u8) -> Self {
        Topic(1u32 << (bit as u32))
    }

    pub const fn from_bits(bits: u32) -> Self {
        Topic(bits)
    }

    pub const fn all() -> Self {
        Topic(u32::MAX)
    }

    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl BitOr for Topic {
    type Output = Topic;

    fn bitor(self, rhs: Topic) -> Topic {
        Topic(self.0 | rhs.0)
    }
}

impl BitOrAssign for Topic {
    fn bitor_assign(&mut self, rhs: Topic) {
        self.0 |= rhs.0;
    }
}

pub const TOPIC_GENERAL: Topic = Topic::bit(0);

impl LineBuilder<'_, '_> {
    pub fn text(&mut self, text: &str) {
        self.text_ref(StrRef::new(text));
    }

    pub fn text_ref(&mut self, text: StrRef<'_>) {
        #[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
        let guard_enabled = guard_logs_enabled();

        #[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
        {
            if guard_enabled && USERS_STR_PROBE.fetch_add(1, Ordering::Relaxed) < PROBE_LIMIT {
                debug_str_probe(text.ptr(), text.len());
            }
        }

        if let Some(bytes) = text.bytes() {
            #[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
            {
                if guard_enabled && USERS_TEXT_FAST.fetch_add(1, Ordering::Relaxed) < PROBE_LIMIT {
                    trace_text_fast(text.ptr(), text.len());
                }
            }
            self.sink.write_bytes(bytes);
            return;
        }

        #[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
        {
            if guard_enabled {
                let fallback_probe = USERS_TEXT_FALLBACK.fetch_add(1, Ordering::Relaxed);
                if fallback_probe < PROBE_LIMIT {
                    trace_text_fallback(text.ptr(), text.len());
                }
                debug_bad_str(text.ptr(), text.len());
            }
        }

        self.sink.write_str("<bad-str ptr=");
        self.hex(text.ptr() as u64);
        self.sink.write_str(" len=");
        self.dec(text.len() as u64);
        self.sink.write_str(">");
    }

    pub fn kv_literal(&mut self, key: &str, value: &str) {
        self.text(key);
        self.sink.write_byte(b'=');
        self.text_ref(StrRef::from(value));
    }

    pub fn kv_hex(&mut self, key: &str, value: u64) {
        self.text(key);
        self.sink.write_byte(b'=');
        self.hex(value);
    }

    pub fn kv_dec(&mut self, key: &str, value: u64) {
        self.text(key);
        self.sink.write_byte(b'=');
        self.dec(value);
    }

    pub fn hex(&mut self, value: u64) {
        #[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
        {
            if HEX_PROBE.fetch_add(1, Ordering::Relaxed) < PROBE_LIMIT {
                debug_ptr(b'H', value as usize);
                debug_ptr(b'R', read_ra());
            }
        }
        self.sink.write_byte(b'0');
        self.sink.write_byte(b'x');
        emit_hex(value, |b| self.sink.write_byte(b));
    }

    pub fn hex_usize(&mut self, value: usize) {
        self.hex(value as u64);
    }

    pub fn dec(&mut self, value: u64) {
        let mut buf = [0u8; 20];
        let mut n = value;
        let mut idx = buf.len();
        if n == 0 {
            idx -= 1;
            buf[idx] = b'0';
        } else {
            while n != 0 {
                idx -= 1;
                buf[idx] = b'0' + (n % 10) as u8;
                n /= 10;
            }
        }
        let slice = &buf[idx..];
        #[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
        {
            let ptr = slice.as_ptr() as usize;
            let len = slice.len();
            if !is_sv39_canonical(ptr) {
                trace_dec_slice(ptr, len, read_ra());
            }
        }
        self.sink.write_bytes(slice);
    }

    pub fn dec_usize(&mut self, value: usize) {
        self.dec(value as u64);
    }

    pub fn fmt(&mut self, args: fmt::Arguments<'_>) {
        fmt::write(&mut self.sink, args).ok();
    }
}

impl sink::Sink<'_> {
    fn write_str(&mut self, s: &str) {
        self.write_bytes(s.as_bytes());
    }
}

#[cfg(all(feature = "sink-userspace", feature = "userspace-linker-bounds", target_arch = "riscv64", target_os = "none"))]
fn overlaps_guard(ptr: usize, end: usize) -> bool {
    extern "C" {
        static __small_data_guard: u8;
        static __image_end: u8;
    }
    let guard_start = core::ptr::addr_of!(__small_data_guard) as usize;
    let guard_end = core::ptr::addr_of!(__image_end) as usize;
    ptr < guard_end && end > guard_start
}

#[cfg(all(feature = "sink-userspace", not(feature = "userspace-linker-bounds"), target_arch = "riscv64", target_os = "none"))]
fn overlaps_guard(_ptr: usize, _end: usize) -> bool {
    false
}

#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
fn log_guard_violation(
    ptr: usize,
    len: usize,
    ra: usize,
    fault: GuardFault,
    level: Level,
    target: &str,
) {
    userspace_putc(b'!');
    for &b in b"guard-str reason=" {
        userspace_putc(b);
    }
    fault.emit_label();
    userspace_putc(b' ');
    for &b in b"ptr=0x" {
        userspace_putc(b);
    }
    emit_hex(ptr as u64, |b| userspace_putc(b));
    userspace_putc(b' ');
    for &b in b"len=0x" {
        userspace_putc(b);
    }
    emit_hex(len as u64, |b| userspace_putc(b));
    userspace_putc(b' ');
    for &b in b"ra=0x" {
        userspace_putc(b);
    }
    emit_hex(ra as u64, |b| userspace_putc(b));
    userspace_putc(b' ');
    for &b in b"level=" {
        userspace_putc(b);
    }
    emit_level_label(level);
    userspace_putc(b' ');
    for &b in b"target=" {
        userspace_putc(b);
    }
    for &b in target.as_bytes().iter().take(16) {
        userspace_putc(b);
    }
    userspace_putc(b'\n');
}

#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
#[allow(dead_code)]
#[derive(Copy, Clone)]
enum GuardFault {
    NonCanonical,
    LenZero,
    LenTooLarge,
    Overflow,
    GuardRange,
}

#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
impl GuardFault {
    fn emit_label(self) {
        match self {
            GuardFault::NonCanonical => emit_literal(b"non-canonical"),
            GuardFault::LenZero => emit_literal(b"len-zero"),
            GuardFault::LenTooLarge => emit_literal(b"len-exceeded"),
            GuardFault::Overflow => emit_literal(b"ptr-overflow"),
            GuardFault::GuardRange => emit_literal(b"guard-range"),
        };
    }
}

fn emit_hex(value: u64, mut emit: impl FnMut(u8)) {
    for shift in (0..(core::mem::size_of::<u64>() * 2)).rev() {
        let nibble = ((value >> (shift * 4)) & 0xf) as u8;
        let ch = if nibble < 10 { b'0' + nibble } else { b'a' + (nibble - 10) };
        emit(ch);
    }
}

#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
const PROBE_LIMIT: usize = 1024;

#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
static USERS_STR_DIAG: AtomicUsize = AtomicUsize::new(0);
#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
static USERS_STR_GOOD: AtomicUsize = AtomicUsize::new(0);
#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
static USERS_TEXT_FAST: AtomicUsize = AtomicUsize::new(0);
#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
static USERS_TEXT_FALLBACK: AtomicUsize = AtomicUsize::new(0);

#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
static USERS_STR_PROBE: AtomicUsize = AtomicUsize::new(0);

#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
static IMAGE_BOUNDS_LOGGED: AtomicBool = AtomicBool::new(false);
#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
static USERS_BAD_PTRS: AtomicUsize = AtomicUsize::new(0);
#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
static HEX_PROBE: AtomicUsize = AtomicUsize::new(0);
#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
static USERS_BOUNDS_ONCE: AtomicBool = AtomicBool::new(false);

#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
fn debug_str_probe(ptr: usize, len: usize) {
    if !guard_logs_enabled() {
        return;
    }
    userspace_putc(b'#');
    for &b in b"probe-str ptr=0x" {
        userspace_putc(b);
    }
    emit_hex(ptr as u64, |b| userspace_putc(b));
    userspace_putc(b' ');
    for &b in b"len=0x" {
        userspace_putc(b);
    }
    emit_hex(len as u64, |b| userspace_putc(b));
    userspace_putc(b'\n');
}

#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
fn debug_bad_str(ptr: usize, len: usize) {
    if !guard_logs_enabled() {
        return;
    }
    userspace_putc(b'!');
    for &b in b"bad-str ptr=0x" {
        userspace_putc(b);
    }
    emit_hex(ptr as u64, |b| userspace_putc(b));
    userspace_putc(b' ');
    for &b in b"len=0x" {
        userspace_putc(b);
    }
    emit_hex(len as u64, |b| userspace_putc(b));
    userspace_putc(b'\n');
}

#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
fn debug_ptr(prefix: u8, value: usize) {
    if !guard_logs_enabled() {
        return;
    }
    userspace_putc(prefix);
    userspace_putc(b'=');
    emit_hex(value as u64, |b| userspace_putc(b));
    userspace_putc(b'\n');
}

#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
fn trace_good_str(ptr: usize, len: usize) {
    if !guard_logs_enabled() {
        return;
    }
    userspace_putc(b'~');
    for &b in b"good-str ptr=0x" {
        userspace_putc(b);
    }
    emit_hex(ptr as u64, |b| userspace_putc(b));
    userspace_putc(b' ');
    for &b in b"len=0x" {
        userspace_putc(b);
    }
    emit_hex(len as u64, |b| userspace_putc(b));
    userspace_putc(b'\n');
}

#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
fn trace_text_fast(ptr: usize, len: usize) {
    if !guard_logs_enabled() {
        return;
    }
    userspace_putc(b'^');
    for &b in b"text-fast ptr=0x" {
        userspace_putc(b);
    }
    emit_hex(ptr as u64, |b| userspace_putc(b));
    userspace_putc(b' ');
    for &b in b"len=0x" {
        userspace_putc(b);
    }
    emit_hex(len as u64, |b| userspace_putc(b));
    userspace_putc(b'\n');
}

#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
fn trace_text_fallback(ptr: usize, len: usize) {
    if !guard_logs_enabled() {
        return;
    }
    userspace_putc(b'^');
    for &b in b"text-fallback ptr=0x" {
        userspace_putc(b);
    }
    emit_hex(ptr as u64, |b| userspace_putc(b));
    userspace_putc(b' ');
    for &b in b"len=0x" {
        userspace_putc(b);
    }
    emit_hex(len as u64, |b| userspace_putc(b));
    userspace_putc(b'\n');
}

#[allow(dead_code)]
fn emit_literal(bytes: &[u8]) {
    for &b in bytes {
        userspace_putc(b);
    }
}

fn level_tag(level: Level) -> u8 {
    match level {
        Level::Error => b'E',
        Level::Warn => b'W',
        Level::Info => b'I',
        Level::Debug => b'D',
        Level::Trace => b'T',
    }
}

#[allow(dead_code)]
fn emit_level_label(level: Level) {
    match level {
        Level::Error => emit_literal(b"ERROR"),
        Level::Warn => emit_literal(b"WARN"),
        Level::Info => emit_literal(b"INFO"),
        Level::Debug => emit_literal(b"DEBUG"),
        Level::Trace => emit_literal(b"TRACE"),
    }
}

#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
fn guard_logs_enabled() -> bool {
    // Default off to keep boot UART clean; opt-in with INIT_LITE_GUARD_LOG=1.
    option_env!("INIT_LITE_GUARD_LOG") == Some("1")
}

// Host/non-RISC-V stub to keep sink-userspace compiling when the feature is
// enabled but we are not building the os-lite target.
#[cfg(all(feature = "sink-userspace", not(all(target_arch = "riscv64", target_os = "none"))))]
fn guard_logs_enabled() -> bool {
    false
}

#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
fn trace_large_write(ptr: usize, len: usize, ra: usize, level: Level, target: &str) {
    if !guard_logs_enabled() {
        return;
    }
    userspace_putc(b'^');
    for &b in b"log-large level=" {
        userspace_putc(b);
    }
    userspace_putc(level_tag(level));
    userspace_putc(b' ');
    for &b in b"target=" {
        userspace_putc(b);
    }
    for &b in target.as_bytes().iter().take(16) {
        userspace_putc(b);
    }
    userspace_putc(b' ');
    for &b in b"log-large ptr=0x" {
        userspace_putc(b);
    }
    emit_hex(ptr as u64, |b| userspace_putc(b));
    userspace_putc(b' ');
    for &b in b"len=0x" {
        userspace_putc(b);
    }
    emit_hex(len as u64, |b| userspace_putc(b));
    userspace_putc(b' ');
    for &b in b"ra=0x" {
        userspace_putc(b);
    }
    emit_hex(ra as u64, |b| userspace_putc(b));
    userspace_putc(b'\n');
}

// Host/non-RISC-V stub.
#[cfg(all(feature = "sink-userspace", not(all(target_arch = "riscv64", target_os = "none"))))]
fn trace_large_write(_ptr: usize, _len: usize, _ra: usize, _level: Level, _target: &str) {}

#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
fn trace_dec_slice(ptr: usize, len: usize, ra: usize) {
    if !guard_logs_enabled() {
        return;
    }
    userspace_putc(b'^');
    for &b in b"dec-slice ptr=0x" {
        userspace_putc(b);
    }
    emit_hex(ptr as u64, |b| userspace_putc(b));
    userspace_putc(b' ');
    for &b in b"len=0x" {
        userspace_putc(b);
    }
    emit_hex(len as u64, |b| userspace_putc(b));
    userspace_putc(b' ');
    for &b in b"ra=0x" {
        userspace_putc(b);
    }
    emit_hex(ra as u64, |b| userspace_putc(b));
    userspace_putc(b'\n');
}

#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
#[inline(always)]
fn read_ra() -> usize {
    let ra: usize;
    unsafe {
        core::arch::asm!("mv {0}, ra", out(reg) ra);
    }
    ra
}

// Host/non-RISC-V stub.
#[cfg(all(feature = "sink-userspace", not(all(target_arch = "riscv64", target_os = "none"))))]
#[inline(always)]
fn read_ra() -> usize {
    0
}

#[cfg(all(feature = "sink-userspace", feature = "userspace-linker-bounds", target_arch = "riscv64", target_os = "none"))]
fn image_bounds() -> (usize, usize) {
    extern "C" {
        static __rodata_start: u8;
        static __small_data_guard: u8;
    }
    let start = core::ptr::addr_of!(__rodata_start) as usize;
    let end = core::ptr::addr_of!(__small_data_guard) as usize;
    #[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
    {
        if !IMAGE_BOUNDS_LOGGED.swap(true, Ordering::Relaxed) {
            debug_ptr(b'S', start);
            debug_ptr(b'E', end);
        }
    }
    (start, end)
}

#[cfg(all(feature = "sink-userspace", not(feature = "userspace-linker-bounds"), target_arch = "riscv64", target_os = "none"))]
fn image_bounds() -> (usize, usize) {
    (0, usize::MAX)
}

#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
fn is_sv39_canonical(addr: usize) -> bool {
    let sign_bit = (addr >> 38) & 1;
    let upper = addr >> 39;
    if sign_bit == 0 {
        upper == 0
    } else {
        upper == (usize::MAX >> 39)
    }
}

#[cfg(feature = "sink-userspace")]
mod sink_userspace {
    use core::fmt;
    use core::sync::atomic::{AtomicUsize, Ordering};

    use super::{emit_hex, guard_violation, level_tag, read_ra, trace_large_write, Level, Topic};

    const LARGE_WRITE_THRESHOLD: usize = 0x1000;
    const LARGE_WRITE_LIMIT: usize = 32;
    static LARGE_WRITE_DIAG: AtomicUsize = AtomicUsize::new(0);
    const SLICE_PROBE_BOOT_LIMIT: usize = 32;
    const SLICE_PROBE_ALERT_THRESHOLD: usize = 0x200;
    static SLICE_PROBE_COUNT: AtomicUsize = AtomicUsize::new(0);

    #[derive(Copy, Clone)]
    enum SliceProbeReason {
        Boot,
        LenAlert,
    }

    impl SliceProbeReason {
        fn label(self) -> &'static str {
            match self {
                SliceProbeReason::Boot => "boot",
                SliceProbeReason::LenAlert => "len-alert",
            }
        }
    }

    fn trace_slice_probe(
        ptr: usize,
        len: usize,
        ra: usize,
        level: Level,
        target: &str,
        reason: SliceProbeReason,
    ) {
        if !crate::guard_logs_enabled() {
            return;
        }
        crate::userspace_putc(b'%');
        for &b in b"slice reason=" {
            crate::userspace_putc(b);
        }
        for &b in reason.label().as_bytes() {
            crate::userspace_putc(b);
        }
        crate::userspace_putc(b' ');
        for &b in b"level=" {
            crate::userspace_putc(b);
        }
        crate::userspace_putc(level_tag(level));
        crate::userspace_putc(b' ');
        for &b in b"target=" {
            crate::userspace_putc(b);
        }
        for &b in target.as_bytes().iter().take(16) {
            crate::userspace_putc(b);
        }
        crate::userspace_putc(b' ');
        for &b in b"ptr=0x" {
            crate::userspace_putc(b);
        }
        emit_hex(ptr as u64, crate::userspace_putc);
        crate::userspace_putc(b' ');
        for &b in b"len=0x" {
            crate::userspace_putc(b);
        }
        emit_hex(len as u64, crate::userspace_putc);
        crate::userspace_putc(b' ');
        for &b in b"ra=0x" {
            crate::userspace_putc(b);
        }
        emit_hex(ra as u64, crate::userspace_putc);
        crate::userspace_putc(b'\n');
    }

    pub struct Sink<'meta> {
        level: Level,
        target: &'meta str,
        _topic: Topic,
        #[cfg(all(feature = "sink-logd", target_arch = "riscv64", target_os = "none"))]
        cap_len: usize,
        #[cfg(all(feature = "sink-logd", target_arch = "riscv64", target_os = "none"))]
        cap_buf: [u8; 320],
    }

    impl<'meta> Sink<'meta> {
        #[allow(dead_code)]
        pub fn new(level: Level, target: &'meta str, topic: Topic) -> Self {
            Self {
                level,
                target,
                _topic: topic,
                #[cfg(all(feature = "sink-logd", target_arch = "riscv64", target_os = "none"))]
                cap_len: 0,
                #[cfg(all(feature = "sink-logd", target_arch = "riscv64", target_os = "none"))]
                cap_buf: [0u8; 320],
            }
        }

        pub fn write_byte(&mut self, byte: u8) {
            crate::userspace_putc(byte);
            #[cfg(all(feature = "sink-logd", target_arch = "riscv64", target_os = "none"))]
            self.capture_byte(byte);
        }

        pub fn write_bytes(&mut self, bytes: &[u8]) {
            let ptr = bytes.as_ptr() as usize;
            let len = bytes.len();
            let ra = read_ra();
            const PROBE_CAP: usize = 32;
            static BYTE_PROBE: AtomicUsize = AtomicUsize::new(0);
            if crate::guard_logs_enabled() && BYTE_PROBE.fetch_add(1, Ordering::Relaxed) < PROBE_CAP
            {
                trace_large_write(ptr, len, ra, self.level, self.target);
            }
            if crate::guard_logs_enabled() {
                crate::userspace_putc(b'&');
                let boot_sample =
                    SLICE_PROBE_COUNT.fetch_add(1, Ordering::Relaxed) < SLICE_PROBE_BOOT_LIMIT;
                let len_alert = len >= SLICE_PROBE_ALERT_THRESHOLD;
                let reason = if len_alert {
                    Some(SliceProbeReason::LenAlert)
                } else if boot_sample {
                    Some(SliceProbeReason::Boot)
                } else {
                    None
                };
                if let Some(reason) = reason {
                    trace_slice_probe(ptr, len, ra, self.level, self.target, reason);
                }
            }
            if guard_violation(ptr, len, ra, self.level, self.target) {
                if LARGE_WRITE_DIAG.fetch_add(1, Ordering::Relaxed) < LARGE_WRITE_LIMIT {
                    trace_large_write(ptr, len, ra, self.level, self.target);
                }
                return;
            }
            if len > LARGE_WRITE_THRESHOLD
                && LARGE_WRITE_DIAG.fetch_add(1, Ordering::Relaxed) < LARGE_WRITE_LIMIT
            {
                trace_large_write(ptr, len, ra, self.level, self.target);
            }
            for &b in bytes {
                self.write_byte(b);
            }
        }

        #[cfg(all(feature = "sink-logd", target_arch = "riscv64", target_os = "none"))]
        fn capture_byte(&mut self, byte: u8) {
            if self.cap_len < self.cap_buf.len() {
                self.cap_buf[self.cap_len] = byte;
                self.cap_len += 1;
            }
        }

        #[cfg(all(feature = "sink-logd", target_arch = "riscv64", target_os = "none"))]
        pub fn capture_bytes(&self) -> &[u8] {
            &self.cap_buf[..self.cap_len]
        }
    }

    impl fmt::Write for Sink<'_> {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            self.write_bytes(s.as_bytes());
            Ok(())
        }
    }
}

#[cfg(all(feature = "sink-logd", target_arch = "riscv64", target_os = "none"))]
mod sink_logd {
    use core::sync::atomic::{AtomicU32, Ordering};

    use nexus_abi::{cap_clone, ipc_recv_v1, MsgHeader, IPC_SYS_NONBLOCK, IPC_SYS_TRUNCATE};
    use nexus_ipc::KernelClient;

    use crate::Level;

    const MAGIC0: u8 = b'L';
    const MAGIC1: u8 = b'O';
    const VERSION: u8 = 1;
    const OP_APPEND: u8 = 1;

    const MAX_SCOPE: usize = 64;
    const MAX_MSG: usize = 256;

    // Cached slots (0 means unknown).
    static LOGD_SEND_SLOT: AtomicU32 = AtomicU32::new(0);
    static REPLY_SEND_SLOT: AtomicU32 = AtomicU32::new(0);
    static REPLY_RECV_SLOT: AtomicU32 = AtomicU32::new(0);

    pub fn try_append(level: Level, target: &str, line: &[u8]) {
        // Best-effort only: logging must not block or panic.
        let (logd_send, reply_send, reply_recv) = match ensure_slots() {
            Some(v) => v,
            None => return,
        };

        let mut frame = [0u8; 4 + 1 + 1 + 2 + 2 + MAX_SCOPE + MAX_MSG];
        let scope = target.as_bytes();
        let scope_len = core::cmp::min(scope.len(), MAX_SCOPE);

        let msg = strip_prefix_and_nl(line);
        let msg_len = core::cmp::min(msg.len(), MAX_MSG);

        // Header
        let mut n = 0usize;
        frame[n] = MAGIC0;
        frame[n + 1] = MAGIC1;
        frame[n + 2] = VERSION;
        frame[n + 3] = OP_APPEND;
        n += 4;
        frame[n] = level_to_logd(level);
        n += 1;
        frame[n] = scope_len as u8;
        n += 1;
        frame[n..n + 2].copy_from_slice(&(msg_len as u16).to_le_bytes());
        n += 2;
        frame[n..n + 2].copy_from_slice(&0u16.to_le_bytes()); // fields_len
        n += 2;
        frame[n..n + scope_len].copy_from_slice(&scope[..scope_len]);
        n += scope_len;
        frame[n..n + msg_len].copy_from_slice(&msg[..msg_len]);
        n += msg_len;

        let moved = match cap_clone(reply_send) {
            Ok(slot) => slot,
            Err(_) => return,
        };

        // Use explicit slots to avoid route queries/allocations per line.
        let client = match KernelClient::new_with_slots(logd_send, reply_recv) {
            Ok(c) => c,
            Err(_) => return,
        };
        let _ = client.send_with_cap_move_wait(&frame[..n], moved, nexus_ipc::Wait::NonBlocking);

        // Drain a few replies to avoid filling the reply inbox under high log volume.
        drain_reply(reply_recv);
    }

    fn ensure_slots() -> Option<(u32, u32, u32)> {
        let send = LOGD_SEND_SLOT.load(Ordering::Relaxed);
        let rs = REPLY_SEND_SLOT.load(Ordering::Relaxed);
        let rr = REPLY_RECV_SLOT.load(Ordering::Relaxed);
        if send != 0 && rs != 0 && rr != 0 {
            return Some((send, rs, rr));
        }

        // Resolve logd route (send slot) and @reply (send+recv).
        let logd = KernelClient::new_for("logd").ok()?;
        let (logd_send, _logd_recv) = logd.slots();

        let reply = KernelClient::new_for("@reply").ok()?;
        let (reply_send, reply_recv) = reply.slots();

        LOGD_SEND_SLOT.store(logd_send, Ordering::Relaxed);
        REPLY_SEND_SLOT.store(reply_send, Ordering::Relaxed);
        REPLY_RECV_SLOT.store(reply_recv, Ordering::Relaxed);

        Some((logd_send, reply_send, reply_recv))
    }

    fn drain_reply(recv_slot: u32) {
        let mut hdr = MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 64];
        for _ in 0..4 {
            match ipc_recv_v1(recv_slot, &mut hdr, &mut buf, IPC_SYS_NONBLOCK | IPC_SYS_TRUNCATE, 0) {
                Ok(_n) => {}
                Err(nexus_abi::IpcError::QueueEmpty) => break,
                Err(_) => break,
            }
        }
    }

    fn strip_prefix_and_nl(line: &[u8]) -> &[u8] {
        let mut s = line;
        if let Some(last) = s.last() {
            if *last == b'\n' {
                s = &s[..s.len().saturating_sub(1)];
            }
        }
        if let Some(pos) = find_bracket_space(s) {
            &s[pos..]
        } else {
            s
        }
    }

    fn find_bracket_space(s: &[u8]) -> Option<usize> {
        // Find the first occurrence of "] " and return the index after it.
        for i in 0..s.len().saturating_sub(1) {
            if s[i] == b']' && s[i + 1] == b' ' {
                return Some(i + 2);
            }
        }
        None
    }

    fn level_to_logd(level: Level) -> u8 {
        match level {
            Level::Error => 0,
            Level::Warn => 1,
            Level::Info => 2,
            Level::Debug => 3,
            Level::Trace => 4,
        }
    }
}

#[inline(never)]
fn guard_violation(_ptr: usize, _len: usize, _ra: usize, _level: Level, _target: &str) -> bool {
    #[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
    {
        let ptr = _ptr;
        let len = _len;
        let ra = _ra;
        let level = _level;
        let target = _target;
        if !is_sv39_canonical(ptr) {
            if guard_logs_enabled() {
                log_guard_violation(ptr, len, ra, GuardFault::NonCanonical, level, target);
            }
            return true;
        }
        if len == 0 {
            return false;
        }
        if len > MAX_SLICE_LEN {
            if guard_logs_enabled() {
                log_guard_violation(ptr, len, ra, GuardFault::LenTooLarge, level, target);
            }
            return true;
        }
        let end = match ptr.checked_add(len) {
            Some(val) => val,
            None => {
                if guard_logs_enabled() {
                    log_guard_violation(ptr, len, ra, GuardFault::Overflow, level, target);
                }
                return true;
            }
        };
        if overlaps_guard(ptr, end) {
            if guard_logs_enabled() {
                log_guard_violation(ptr, len, ra, GuardFault::GuardRange, level, target);
            }
            return true;
        }
        // Reject slices that fall outside the mapped image; otherwise we may
        // read from unmapped stack/guard pages and fault before logging.
        let (start, limit) = image_bounds();
        let in_range = ptr >= start && end <= limit;
        if !in_range {
            if guard_logs_enabled() {
                log_guard_violation(ptr, len, ra, GuardFault::GuardRange, level, target);
            }
            return true;
        }
    }
    false
}

#[cfg(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none"))]
fn userspace_putc(byte: u8) {
    // Use the canonical syscall wrapper to avoid subtle inline-asm ABI hazards.
    let _ = nexus_abi::debug_putc(byte);
}

#[cfg(not(all(feature = "sink-userspace", target_arch = "riscv64", target_os = "none")))]
fn userspace_putc(_byte: u8) {}

#[cfg(feature = "sink-kernel")]
mod sink_kernel {
    use core::fmt;

    use super::{Level, Topic};

    pub struct Sink<'meta> {
        #[allow(unused)]
        level: Level,
        #[allow(unused)]
        target: &'meta str,
        #[allow(unused)]
        topic: Topic,
    }

    impl<'meta> Sink<'meta> {
        #[allow(dead_code)]
        pub fn new(level: Level, target: &'meta str, topic: Topic) -> Self {
            Self { level, target, topic }
        }

        pub fn write_byte(&mut self, byte: u8) {
            unsafe {
                const UART_BASE: usize = 0x1000_0000;
                const UART_TX: usize = 0x0;
                const UART_LSR: usize = 0x5;
                const LSR_TX_IDLE: u8 = 1 << 5;

                while core::ptr::read_volatile((UART_BASE + UART_LSR) as *const u8) & LSR_TX_IDLE
                    == 0
                {}
                core::ptr::write_volatile((UART_BASE + UART_TX) as *mut u8, byte);
            }
        }

        pub fn write_bytes(&mut self, bytes: &[u8]) {
            for &b in bytes {
                self.write_byte(b);
            }
        }
    }

    impl fmt::Write for Sink<'_> {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            self.write_bytes(s.as_bytes());
            Ok(())
        }
    }
}

#[cfg(feature = "sink-userspace")]
use sink_userspace as sink;

#[cfg(all(not(feature = "sink-userspace"), feature = "sink-kernel"))]
use sink_kernel as sink;

#[cfg(all(not(feature = "sink-userspace"), not(feature = "sink-kernel")))]
mod sink {
    compile_error!("nexus-log requires enabling either `sink-userspace` or `sink-kernel`.");
}
