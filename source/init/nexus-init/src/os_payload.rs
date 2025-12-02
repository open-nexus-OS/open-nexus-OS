extern crate alloc;

use alloc::vec::Vec;
use core::fmt;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use nexus_abi::{self, AbiError, IpcError};
use nexus_log::{self, Level, LineBuilder, StrRef};

const MAX_LOG_STR_LEN: usize = 512;

extern "C" {
    static __rodata_start: u8;
    static __rodata_end: u8;
    static __data_start: u8;
    static __data_end: u8;
    static __small_data_guard: u8;
    static __image_end: u8;
}

/// Prepackaged service image embedded into the init payload.
pub struct ServiceImage {
    /// Logical service name used for logging.
    pub name: &'static str,
    /// Raw ELF bytes for the service.
    pub elf: &'static [u8],
    /// Number of stack pages to allocate for the service.
    pub stack_pages: u64,
    /// RISC-V global pointer used when spawning the task.
    pub global_pointer: u64,
}

/// Errors produced while materialising service images.
#[derive(Debug, Clone)]
pub enum InitError {
    /// Kernel ABI call failed.
    Abi(AbiError),
    /// IPC syscall failed.
    Ipc(IpcError),
    /// Malformed ELF payload.
    Elf(&'static str),
    /// Segment mapping violation (overflow, overlap, etc.).
    Map(&'static str),
    /// Requested service carried an empty image.
    MissingElf,
}

impl fmt::Display for InitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InitError::Abi(code) => write!(f, "abi error: {:?}", code),
            InitError::Ipc(code) => write!(f, "ipc error: {:?}", code),
            InitError::Elf(msg) => write!(f, "elf error: {msg}"),
            InitError::Map(msg) => write!(f, "map error: {msg}"),
            InitError::MissingElf => write!(f, "no elf payload"),
        }
    }
}

/// Callback invoked once the bootstrapper reaches `init: ready`.
pub struct ReadyNotifier<F: FnOnce() + Send>(F);

impl<F: FnOnce() + Send> ReadyNotifier<F> {
    /// Create a new notifier that will run `func` when signalled.
    pub fn new(func: F) -> Self {
        Self(func)
    }

    /// Execute the wrapped callback.
    pub fn notify(self) {
        (self.0)();
    }
}

mod log_topics {
    use nexus_log::{Topic, TOPIC_GENERAL};

    pub const GENERAL: Topic = TOPIC_GENERAL;
    pub const SERVICE_META: Topic = Topic::bit(1);
    pub const PROBE: Topic = Topic::bit(2);

    pub const DEFAULT_MASK: Topic = GENERAL;

    fn matches_general(slice: &[u8]) -> bool {
        slice.len() == 7
            && slice[0] == b'g'
            && slice[1] == b'e'
            && slice[2] == b'n'
            && slice[3] == b'e'
            && slice[4] == b'r'
            && slice[5] == b'a'
            && slice[6] == b'l'
    }

    fn matches_svc_meta(slice: &[u8]) -> bool {
        slice.len() == 8
            && slice[0] == b's'
            && slice[1] == b'v'
            && slice[2] == b'c'
            && slice[3] == b'-'
            && slice[4] == b'm'
            && slice[5] == b'e'
            && slice[6] == b't'
            && slice[7] == b'a'
    }

    fn matches_probe(slice: &[u8]) -> bool {
        slice.eq_ignore_ascii_case(b"probe")
    }

    pub fn parse_spec(spec: &[u8]) -> Topic {
        fn apply_token(mask: &mut Topic, bytes: &[u8], start: usize, end: usize) {
            let mut lo = start;
            let mut hi = end;
            while lo < hi && bytes[lo].is_ascii_whitespace() {
                lo += 1;
            }
            while hi > lo && bytes[hi - 1].is_ascii_whitespace() {
                hi -= 1;
            }
            if lo == hi {
                return;
            }
            let slice = &bytes[lo..hi];
            if matches_general(slice) {
                *mask |= GENERAL;
                return;
            }
            if matches_svc_meta(slice) {
                *mask |= SERVICE_META;
                return;
            }
            if matches_probe(slice) {
                *mask |= PROBE;
            }
        }

        let mut mask = Topic::empty();
        let mut start = 0usize;
        for (idx, &byte) in spec.iter().enumerate() {
            if byte == b',' {
                apply_token(&mut mask, spec, start, idx);
                start = idx + 1;
            }
        }
        apply_token(&mut mask, spec, start, spec.len());

        if mask.is_empty() {
            DEFAULT_MASK
        } else {
            mask | GENERAL
        }
    }
}

type Result<T> = core::result::Result<T, InitError>;

const USER_STACK_TOP: u64 = 0x4000_0000;
const BOOTSTRAP_SLOT: u32 = 0;
const PAGE_SIZE: u64 = 4096;
const MAP_FLAG_USER: u32 = 1 << 0;
const PROT_READ: u32 = 1 << 0;
const PROT_WRITE: u32 = 1 << 1;
const PROT_EXEC: u32 = 1 << 2;
static PROBE_ENABLED: AtomicBool = AtomicBool::new(false);
fn configure_log_topics() {
    let mask = match option_env!("INIT_LITE_LOG_TOPICS") {
        Some(spec) if !spec.is_empty() => log_topics::parse_spec(spec.as_bytes()),
        _ => log_topics::DEFAULT_MASK,
    };
    nexus_log::set_topic_mask(mask);
    let probe = (mask.bits() & log_topics::PROBE.bits()) != 0;
    PROBE_ENABLED.store(probe, Ordering::Relaxed);
    debug_write_bytes(b"log topics mask=0x");
    debug_write_hex(mask.bits() as usize);
    debug_write_byte(b'\n');
}

const GUARD_STR_PROBE_LIMIT: usize = 128;
static GUARD_STR_PROBE_COUNT: AtomicUsize = AtomicUsize::new(0);
const SEGMENT_PLAN_PROBE_LIMIT: usize = 32;
const SEGMENT_CHUNK_PROBE_LIMIT: usize = 512;
static SEGMENT_PLAN_PROBE_COUNT: AtomicUsize = AtomicUsize::new(0);
static SEGMENT_CHUNK_PROBE_COUNT: AtomicUsize = AtomicUsize::new(0);

fn debug_write_byte(byte: u8) {
    let _ = nexus_abi::debug_putc(byte);
}

fn debug_write_bytes(bytes: &[u8]) {
    for &b in bytes {
        debug_write_byte(b);
    }
}

fn debug_write_str(s: &str) {
    debug_write_bytes(s.as_bytes());
}

fn debug_write_hex_byte(byte: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let hi = HEX[(byte >> 4) as usize];
    let lo = HEX[(byte & 0xF) as usize];
    debug_write_byte(hi);
    debug_write_byte(lo);
}

fn debug_write_hex(value: usize) {
    const NIBBLES: usize = core::mem::size_of::<usize>() * 2;
    for shift in (0..NIBBLES).rev() {
        let nibble = ((value >> (shift * 4)) & 0xF) as u8;
        let ch = if nibble < 10 {
            b'0' + nibble
        } else {
            b'a' + (nibble - 10)
        };
        debug_write_byte(ch);
    }
}

fn probe_debug_write_words() {
    if !probes_enabled() {
        return;
    }
    const PROBE_WORDS: usize = 4;
    let base = nexus_abi::debug_write as usize;
    debug_write_bytes(b"!dbg-probe base=0x");
    debug_write_hex(base);
    debug_write_byte(b'\n');
    unsafe {
        for idx in 0..PROBE_WORDS {
            let ptr = (base + idx * core::mem::size_of::<u32>()) as *const u32;
            let word = core::ptr::read_unaligned(ptr) as usize;
            debug_write_bytes(b"!dbg-word idx=0x");
            debug_write_hex(idx);
            debug_write_bytes(b" val=0x");
            debug_write_hex(word);
            debug_write_byte(b'\n');
        }
    }
}

fn raw_probe_str(tag: &str, value: &str) {
    if !probes_enabled() {
        return;
    }
    debug_write_byte(b'^');
    debug_write_str(tag);
    debug_write_bytes(b" ptr=0x");
    debug_write_hex(value.as_ptr() as usize);
    debug_write_bytes(b" len=0x");
    debug_write_hex(value.len());
    if !value.is_empty() {
        debug_write_bytes(b" data=");
        let mut count = value.len();
        if count > 8 {
            count = 8;
        }
        let bytes = value.as_bytes();
        for (idx, byte) in bytes.iter().take(count).enumerate() {
            debug_write_hex_byte(*byte);
            if idx + 1 != count {
                debug_write_byte(b' ');
            }
        }
    }
    debug_write_byte(b'\n');
}

fn log_s5(tag: &str, value: usize) {
    debug_write_byte(b'@');
    debug_write_str(tag);
    debug_write_bytes(b" s5=0x");
    debug_write_hex(value);
    debug_write_byte(b'\n');
}

#[inline(always)]
fn read_s5() -> usize {
    let value: usize;
    unsafe {
        core::arch::asm!(
            "mv {out}, s5",
            out = out(reg) value,
            options(nomem, nostack)
        );
    }
    value
}

fn log_str_ptr(tag: &str, value: &str) {
    raw_probe_str(tag, value);
    nexus_log::trace_topic("init", log_topics::SERVICE_META, |line| {
        line.text_ref(StrRef::from(tag));
        line.text(" ptr=");
        line.hex(value.as_ptr() as u64);
        line.text(" len=");
        line.dec(value.len() as u64);
    });
}

fn log_invalid_str(tag: &str, ptr: usize, len: usize) {
    nexus_log::trace_topic("init", log_topics::SERVICE_META, |line| {
        line.text_ref(StrRef::from(tag));
        line.text(" ptr=");
        line.hex(ptr as u64);
        line.text(" len=");
        line.dec(len as u64);
        line.text(" invalid-range");
    });
}

fn trace_guard_str(event: &str, ptr: usize, len: usize, truncated: bool) {
    if !probes_enabled() {
        return;
    }
    if GUARD_STR_PROBE_COUNT.fetch_add(1, Ordering::Relaxed) >= GUARD_STR_PROBE_LIMIT {
        return;
    }
    debug_write_byte(b'!');
    debug_write_str(event);
    if truncated {
        debug_write_bytes(b" trunc");
    }
    debug_write_bytes(b" ptr=0x");
    debug_write_hex(ptr);
    debug_write_bytes(b" len=0x");
    debug_write_hex(len);
    debug_write_byte(b'\n');
}

fn trace_segment_plan(
    idx: usize,
    base: u64,
    map_len: usize,
    page_offset: usize,
    data_len: usize,
    memsz: u64,
    prot: u32,
) {
    if !probes_enabled() {
        return;
    }
    if SEGMENT_PLAN_PROBE_COUNT.fetch_add(1, Ordering::Relaxed) >= SEGMENT_PLAN_PROBE_LIMIT {
        return;
    }
    debug_write_byte(b'!');
    debug_write_bytes(b"seg-plan idx=0x");
    debug_write_hex(idx);
    debug_write_bytes(b" base=0x");
    debug_write_hex(base as usize);
    debug_write_bytes(b" map_len=0x");
    debug_write_hex(map_len);
    debug_write_bytes(b" page_off=0x");
    debug_write_hex(page_offset);
    debug_write_bytes(b" data_len=0x");
    debug_write_hex(data_len);
    debug_write_bytes(b" memsz=0x");
    debug_write_hex(memsz as usize);
    debug_write_bytes(b" prot=0x");
    debug_write_hex(prot as usize);
    debug_write_byte(b'\n');
}

fn trace_segment_chunk(tag: &str, idx: usize, offset: usize, len: usize) {
    if !probes_enabled() {
        return;
    }
    if SEGMENT_CHUNK_PROBE_COUNT.fetch_add(1, Ordering::Relaxed) >= SEGMENT_CHUNK_PROBE_LIMIT {
        return;
    }
    debug_write_byte(b'!');
    debug_write_bytes(b"seg-");
    debug_write_str(tag);
    debug_write_bytes(b" idx=0x");
    debug_write_hex(idx);
    debug_write_bytes(b" off=0x");
    debug_write_hex(offset);
    debug_write_bytes(b" len=0x");
    debug_write_hex(len);
    debug_write_byte(b'\n');
}

fn probes_enabled() -> bool {
    PROBE_ENABLED.load(Ordering::Relaxed)
}

fn section_range(start: &u8, end: &u8) -> core::ops::Range<usize> {
    let base = start as *const u8 as usize;
    let end = end as *const u8 as usize;
    base..end
}

fn section_contains(range: &core::ops::Range<usize>, ptr: usize, len: usize) -> bool {
    if range.is_empty() {
        return false;
    }
    let end = match ptr.checked_add(len) {
        Some(end) => end,
        None => return false,
    };
    ptr >= range.start && end <= range.end
}

fn is_user_str_valid(ptr: usize, len: usize) -> bool {
    if len == 0 || len > MAX_LOG_STR_LEN {
        return false;
    }
    let ro_range = unsafe { section_range(&__rodata_start, &__rodata_end) };
    let data_range = unsafe { section_range(&__data_start, &__data_end) };
    section_contains(&ro_range, ptr, len) || section_contains(&data_range, ptr, len)
}

const LOG_CANARY: u64 = 0x96d1_28f4_a55a_d00d;
const SEGMENT_BOUNCE_LEN: usize = 0x1000;

struct GuardedBounce {
    head: u64,
    buf: [u8; SEGMENT_BOUNCE_LEN],
    tail: u64,
}

impl GuardedBounce {
    fn new() -> Self {
        Self {
            head: LOG_CANARY,
            buf: [0u8; SEGMENT_BOUNCE_LEN],
            tail: LOG_CANARY,
        }
    }

    fn clear(&mut self) {
        for byte in self.buf.iter_mut() {
            *byte = 0;
        }
    }

    fn assert_intact(&self, tag: &str) {
        if self.head != LOG_CANARY || self.tail != LOG_CANARY {
            panic!("guarded bounce buffer corrupted ({tag})");
        }
    }

    fn write_data_chunk(
        &mut self,
        vmo: nexus_abi::Handle,
        offset: usize,
        data: &[u8],
        tag: &str,
    ) -> Result<()> {
        debug_assert!(data.len() <= self.buf.len());
        self.clear();
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
        }
        self.assert_intact(tag);
        nexus_abi::vmo_write(vmo, offset, &self.buf[..data.len()]).map_err(InitError::Ipc)
    }

    fn write_zero_chunk(
        &mut self,
        vmo: nexus_abi::Handle,
        offset: usize,
        len: usize,
    ) -> Result<()> {
        debug_assert!(len <= self.buf.len());
        self.clear();
        self.assert_intact("zero-chunk");
        nexus_abi::vmo_write(vmo, offset, &self.buf[..len]).map_err(InitError::Ipc)
    }
}

struct ServiceNameGuard<'a> {
    value: Option<&'a str>,
    ptr: usize,
    len: usize,
}

impl<'a> ServiceNameGuard<'a> {
    fn new(raw: &'a str) -> Self {
        let ptr = raw.as_ptr() as usize;
        let len = raw.len();
        let value = if is_user_str_valid(ptr, len) {
            trace_guard_str("svc-name", ptr, len, false);
            Some(raw)
        } else {
            trace_guard_str("svc-name-invalid", ptr, len, false);
            None
        };
        Self { value, ptr, len }
    }

    fn log_probe(&self, tag: &str) {
        if let Some(value) = self.value {
            log_str_ptr(tag, value);
        } else {
            log_invalid_str(tag, self.ptr, self.len);
        }
    }

    fn trace_metadata(&self) {
        nexus_log::trace_topic("init", log_topics::SERVICE_META, |line| {
            line.text("svc meta name_ptr=");
            line.hex(self.ptr as u64);
            line.text(" len=");
            line.dec(self.len as u64);
            if let Some(value) = self.value {
                let mut preview = 0u64;
                for &b in value.as_bytes().iter().take(8) {
                    preview = (preview << 8) | b as u64;
                }
                line.text(" bytes=");
                line.hex(preview);
            } else {
                line.text(" bytes=invalid");
            }
        });
    }

    fn write(&self, line: &mut nexus_log::LineBuilder<'_, '_>) {
        if let Some(value) = self.value {
            raw_probe_str("guard-write", value);
            line.text_ref(StrRef::from_unchecked(value));
        } else {
            line.text("[svc@0x");
            line.hex(self.ptr as u64);
            line.text("/");
            line.hex(self.len as u64);
            line.text("]");
        }
    }
}

/// Map, zero, and spawn every service image once, signalling `notifier` on completion.
pub fn bootstrap_service_images<F>(
    images: &'static [ServiceImage],
    notifier: ReadyNotifier<F>,
) -> Result<()>
where
    F: FnOnce() + Send,
{
    probe_debug_write_words();
    configure_log_topics();
    nexus_log::set_max_level(Level::Trace);
    log_str_ptr("init-msg", "init: start");
    nexus_log::info_static("init", "init: start");
    log_image_bounds();

    if images.is_empty() {
        nexus_log::warn_static("init", "no services configured");
    }

    for image in images {
        let name = ServiceNameGuard::new(image.name);
        name.log_probe("svc-name");
        name.trace_metadata();
        nexus_log::info("init", |line| {
            line.text("init: start ");
            name.write(line);
        });
        match spawn_service(image, &name) {
            Ok(pid) => {
                nexus_log::info("init", |line| {
                    line.text("spawn name=");
                    name.write(line);
                    line.text(" pid=");
                    line.dec(pid as u64);
                });
                nexus_log::info("init", |line| {
                    line.text("init: up ");
                    name.write(line);
                });
                nexus_log::debug("init", |line| {
                    line.text("init-up name=");
                    name.write(line);
                    line.text(" pid=");
                    line.dec(pid as u64);
                });
            }
            Err(err) => nexus_log::warn("init", |line| {
                line.text("spawn-fail name=");
                name.write(line);
                line.text(" reason=");
                describe_init_error(line, &err);
            }),
        }
        let _ = nexus_abi::yield_();
    }

    notifier.notify();
    nexus_log::info_static("init", "init: ready");
    Ok(())
}

/// Same as [`bootstrap_service_images`] but keeps the init task alive forever.
pub fn service_main_loop_images<F>(
    images: &'static [ServiceImage],
    notifier: ReadyNotifier<F>,
) -> Result<()>
where
    F: FnOnce() + Send,
{
    bootstrap_service_images(images, notifier)?;
    loop {
        let _ = nexus_abi::yield_();
    }
}

fn spawn_service(image: &ServiceImage, name: &ServiceNameGuard<'_>) -> Result<u32> {
    if image.elf.is_empty() {
        return Err(InitError::MissingElf);
    }

    let parsed = parse_elf(image.elf)?;
    nexus_log::debug("init", |line| {
        line.text("prepare name=");
        name.write(line);
        line.text(" entry=");
        line.hex(parsed.entry);
    });

    let as_handle = nexus_abi::as_create().map_err(InitError::Abi)?;
    map_segments(as_handle, &parsed.segments)?;

    let stack_pages = image.stack_pages.max(1);
    let (stack_handle, sp) = map_stack(as_handle, stack_pages)?;
    let mut stack_guard = Some(stack_handle);

    nexus_log::debug("init", |line| {
        line.text("spawn name=");
        name.write(line);
        line.text(" sp=");
        line.hex(sp);
        line.text(" slot=");
        line.dec(BOOTSTRAP_SLOT as u64);
    });

    let pid = match nexus_abi::spawn(
        parsed.entry,
        sp,
        as_handle,
        BOOTSTRAP_SLOT,
        image.global_pointer,
    ) {
        Ok(pid) => pid,
        Err(err) => {
            release_optional(&mut stack_guard);
            return Err(InitError::Abi(err));
        }
    };

    release_optional(&mut stack_guard);

    Ok(pid)
}

fn release_vmo(handle: nexus_abi::Handle) {
    match nexus_abi::vmo_destroy(handle) {
        Ok(()) => {}
        Err(AbiError::InvalidSyscall) | Err(AbiError::Unsupported) => {}
        Err(err) => nexus_log::warn("init", |line| {
            line.text("vmo-destroy failed err=");
            line.text(abi_error_label(err));
        }),
    }
}

fn release_optional(handle: &mut Option<nexus_abi::Handle>) {
    if let Some(h) = handle.take() {
        release_vmo(h);
    }
}

struct ParsedElf<'a> {
    entry: u64,
    segments: Vec<Segment<'a>>,
}

struct Segment<'a> {
    vaddr: u64,
    memsz: u64,
    flags: u32,
    data: &'a [u8],
}

fn parse_elf(bytes: &[u8]) -> Result<ParsedElf<'_>> {
    if bytes.len() < 64 {
        return Err(InitError::Elf("elf header truncated"));
    }
    if &bytes[0..4] != b"\x7fELF" {
        return Err(InitError::Elf("bad magic"));
    }
    if bytes[4] != 2 {
        return Err(InitError::Elf("unsupported class"));
    }
    if bytes[5] != 1 {
        return Err(InitError::Elf("unsupported endianness"));
    }

    let entry = read_u64(bytes, 24).ok_or(InitError::Elf("missing entry"))?;
    let phoff = read_u64(bytes, 32).ok_or(InitError::Elf("missing phoff"))?;
    let phentsize = read_u16(bytes, 54).ok_or(InitError::Elf("missing phentsize"))? as usize;
    let phnum = read_u16(bytes, 56).ok_or(InitError::Elf("missing phnum"))? as usize;

    if phentsize < 56 {
        return Err(InitError::Elf("phentsize too small"));
    }
    if phnum == 0 {
        return Err(InitError::Elf("no program headers"));
    }

    let mut segments = Vec::new();
    for index in 0..phnum {
        let offset = (phoff as usize)
            .checked_add(
                index
                    .checked_mul(phentsize)
                    .ok_or(InitError::Elf("ph overflow"))?,
            )
            .ok_or(InitError::Elf("ph overflow"))?;
        if offset + phentsize > bytes.len() {
            return Err(InitError::Elf("program header truncated"));
        }
        let p_type = read_u32(bytes, offset).ok_or(InitError::Elf("missing p_type"))?;
        const PT_LOAD: u32 = 1;
        if p_type != PT_LOAD {
            continue;
        }
        let p_flags = read_u32(bytes, offset + 4).ok_or(InitError::Elf("missing p_flags"))?;
        let p_offset = read_u64(bytes, offset + 8).ok_or(InitError::Elf("missing p_offset"))?;
        let p_vaddr = read_u64(bytes, offset + 16).ok_or(InitError::Elf("missing p_vaddr"))?;
        let p_filesz = read_u64(bytes, offset + 32).ok_or(InitError::Elf("missing p_filesz"))?;
        let p_memsz = read_u64(bytes, offset + 40).ok_or(InitError::Elf("missing p_memsz"))?;
        let p_align = read_u64(bytes, offset + 48).ok_or(InitError::Elf("missing p_align"))?;

        if p_memsz == 0 {
            continue;
        }
        if p_filesz > p_memsz {
            return Err(InitError::Elf("filesz exceeds memsz"));
        }
        if p_align != 0 && p_align % PAGE_SIZE != 0 {
            return Err(InitError::Elf("segment alignment violation"));
        }

        let data = if p_filesz == 0 {
            &bytes[0..0]
        } else {
            let start = p_offset as usize;
            let end = start
                .checked_add(p_filesz as usize)
                .ok_or(InitError::Elf("file range overflow"))?;
            if end > bytes.len() {
                return Err(InitError::Elf("segment data truncated"));
            }
            &bytes[start..end]
        };

        segments.push(Segment {
            vaddr: p_vaddr,
            memsz: p_memsz,
            flags: p_flags,
            data,
        });
    }

    if segments.is_empty() {
        return Err(InitError::Elf("no loadable segments"));
    }

    Ok(ParsedElf { entry, segments })
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    bytes
        .get(offset..offset + 2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    bytes
        .get(offset..offset + 4)
        .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
}

fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    bytes.get(offset..offset + 8).map(|chunk| {
        u64::from_le_bytes([
            chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
        ])
    })
}

#[derive(Copy, Clone)]
enum RangeKind {
    Segment,
    Guard,
}

impl RangeKind {
    fn label(self) -> &'static str {
        match self {
            Self::Segment => "segment",
            Self::Guard => "guard",
        }
    }
}

#[derive(Copy, Clone)]
struct RangeEntry {
    base: u64,
    end: u64,
    kind: RangeKind,
}

struct RangeTracker {
    entries: Vec<RangeEntry>,
}

impl RangeTracker {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn ensure_available(&self, base: u64, len: u64, kind: RangeKind) -> Result<()> {
        let end = base
            .checked_add(len)
            .ok_or(InitError::Map("range end overflow"))?;
        for entry in &self.entries {
            let guard_conflict =
                matches!(kind, RangeKind::Guard) || matches!(entry.kind, RangeKind::Guard);
            if guard_conflict && ranges_overlap(base, end, entry.base, entry.end) {
                log_range_conflict(kind, base, end, entry);
                return Err(InitError::Map("guard range overlap"));
            }
        }
        Ok(())
    }

    fn insert(&mut self, base: u64, len: u64, kind: RangeKind) {
        let end = base
            .checked_add(len)
            .expect("range end overflow after prior validation");
        self.entries.push(RangeEntry { base, end, kind });
    }
}

fn ranges_overlap(a_base: u64, a_end: u64, b_base: u64, b_end: u64) -> bool {
    !(a_end <= b_base || b_end <= a_base)
}

fn log_range_conflict(kind: RangeKind, base: u64, end: u64, other: &RangeEntry) {
    debug_write_bytes(b"guard-conflict new=");
    debug_write_str(kind.label());
    debug_write_bytes(b" base=0x");
    debug_write_hex(base as usize);
    debug_write_bytes(b" end=0x");
    debug_write_hex(end as usize);
    debug_write_bytes(b" other=");
    debug_write_str(other.kind.label());
    debug_write_bytes(b" base=0x");
    debug_write_hex(other.base as usize);
    debug_write_bytes(b" end=0x");
    debug_write_hex(other.end as usize);
    debug_write_byte(b'\n');
    nexus_log::error("init", |line| {
        line.text("guard-conflict new=");
        line.text(kind.label());
        line.text(" base=");
        line.hex(base);
        line.text(" end=");
        line.hex(end);
        line.text(" other=");
        line.text(other.kind.label());
        line.text(" base=");
        line.hex(other.base);
        line.text(" end=");
        line.hex(other.end);
    });
}

fn map_segments(as_handle: nexus_abi::AsHandle, segments: &[Segment<'_>]) -> Result<()> {
    let mut tracker = RangeTracker::new();
    for (idx, segment) in segments.iter().enumerate() {
        map_segment(as_handle, &mut tracker, segment, idx)?;
    }
    Ok(())
}

const SMALL_DATA_GUARD: u64 = 0x20_000;
static GUARD_VMO: AtomicU32 = AtomicU32::new(0);

fn map_segment(
    as_handle: nexus_abi::AsHandle,
    tracker: &mut RangeTracker,
    segment: &Segment<'_>,
    index: usize,
) -> Result<()> {
    if segment.memsz == 0 {
        return Ok(());
    }

    let prot = prot_from_flags(segment.flags)?;

    let page_mask = PAGE_SIZE - 1;
    let map_base = segment.vaddr & !page_mask;
    let page_offset = segment
        .vaddr
        .checked_sub(map_base)
        .ok_or(InitError::Map("address underflow"))?;
    let total = segment
        .memsz
        .checked_add(page_offset)
        .ok_or(InitError::Map("segment size overflow"))?;
    let map_len = align_up(total, PAGE_SIZE).ok_or(InitError::Map("segment length overflow"))?;
    let map_len_usize = map_len as usize;
    let page_offset_usize = page_offset as usize;
    trace_segment_plan(
        index,
        map_base,
        map_len_usize,
        page_offset_usize,
        segment.data.len(),
        segment.memsz,
        prot,
    );
    nexus_log::debug("init", |line| {
        line.text("seg-plan idx=");
        line.dec(index as u64);
        line.text(" base=");
        line.hex(map_base);
        line.text(" map_len=");
        line.hex(map_len);
        line.text(" page_off=");
        line.hex(page_offset);
        line.text(" data_len=");
        line.dec(segment.data.len() as u64);
        line.text(" memsz=");
        line.hex(segment.memsz);
        line.text(" prot=");
        line.hex(prot as u64);
    });

    let vmo = nexus_abi::vmo_create(map_len_usize).map_err(InitError::Ipc)?;
    let mut bounce = GuardedBounce::new();

    if probes_enabled() {
        debug_write_bytes(b"map-image ptr=0x");
        debug_write_hex(bounce.buf.as_ptr() as usize);
        debug_write_bytes(b" len=0x");
        debug_write_hex(SEGMENT_BOUNCE_LEN);
        debug_write_byte(b'\n');
    }
    nexus_log::error("init", |line| {
        line.text("map-image bounce len=0x");
        line.hex(SEGMENT_BOUNCE_LEN as u64);
        line.text(" segment len=0x");
        line.hex(map_len);
    });

    let zero_result = write_zero_filled_image(&mut bounce, vmo, map_len_usize, index);
    if let Err(err) = zero_result {
        let _ = nexus_abi::vmo_destroy(vmo);
        return Err(err);
    }

    if !segment.data.is_empty() {
        let data_len = segment.data.len();
        if page_offset_usize + data_len > map_len_usize {
            let _ = nexus_abi::vmo_destroy(vmo);
            return Err(InitError::Map("segment image overflow"));
        }
        if let Err(err) =
            write_segment_bytes(&mut bounce, vmo, page_offset_usize, segment.data, index)
        {
            let _ = nexus_abi::vmo_destroy(vmo);
            return Err(err);
        }
    }
    nexus_log::trace("init", |line| {
        line.text("map request base=");
        line.hex(map_base);
        line.text(" len=");
        line.hex(map_len);
        line.text(" prot=");
        line.hex(prot as u64);
    });
    tracker.ensure_available(map_base, map_len, RangeKind::Segment)?;
    if let Err(err) = nexus_abi::as_map(as_handle, vmo, map_base, map_len, prot, MAP_FLAG_USER) {
        let _ = nexus_abi::vmo_destroy(vmo);
        return Err(InitError::Abi(err));
    }
    tracker.insert(map_base, map_len, RangeKind::Segment);
    nexus_log::debug("init", |line| {
        line.text("map base=");
        line.hex(map_base);
        line.text(" len=");
        line.dec(map_len);
        line.text(" flags=");
        line.hex(prot as u64);
    });
    release_vmo(vmo);
    if prot & PROT_WRITE != 0 {
        install_guard_pages(as_handle, tracker, map_base, map_len)?;
    }
    Ok(())
}

fn write_zero_filled_image(
    bounce: &mut GuardedBounce,
    vmo: nexus_abi::Handle,
    len: usize,
    idx: usize,
) -> Result<()> {
    let mut offset = 0usize;
    while offset < len {
        let chunk_len = core::cmp::min(len - offset, SEGMENT_BOUNCE_LEN);
        trace_segment_chunk("zero", idx, offset, chunk_len);
        bounce.write_zero_chunk(vmo, offset, chunk_len)?;
        offset += chunk_len;
    }
    Ok(())
}

fn write_segment_bytes(
    bounce: &mut GuardedBounce,
    vmo: nexus_abi::Handle,
    start: usize,
    data: &[u8],
    idx: usize,
) -> Result<()> {
    let mut written = 0usize;
    while written < data.len() {
        let chunk_len = core::cmp::min(data.len() - written, SEGMENT_BOUNCE_LEN);
        let slice = &data[written..written + chunk_len];
        trace_segment_chunk("data", idx, start + written, chunk_len);
        bounce.write_data_chunk(vmo, start + written, slice, "segment-chunk")?;
        written += chunk_len;
    }
    nexus_log::debug("init", |line| {
        line.text("seg-bytes idx=");
        line.dec(idx as u64);
        line.text(" start=");
        line.hex(start as u64);
        line.text(" len=");
        line.dec(data.len() as u64);
    });
    Ok(())
}

fn guard_vmo_handle() -> Result<nexus_abi::Handle> {
    let handle = GUARD_VMO.load(Ordering::Acquire);
    if handle != 0 {
        return Ok(handle);
    }
    let new_handle = nexus_abi::vmo_create(SMALL_DATA_GUARD as usize).map_err(InitError::Ipc)?;
    match GUARD_VMO.compare_exchange(0, new_handle, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => Ok(new_handle),
        Err(existing) => {
            let _ = nexus_abi::vmo_destroy(new_handle);
            Ok(existing)
        }
    }
}

fn install_guard_pages(
    as_handle: nexus_abi::AsHandle,
    tracker: &mut RangeTracker,
    map_base: u64,
    map_len: u64,
) -> Result<()> {
    let guard_base = map_base
        .checked_add(map_len)
        .ok_or(InitError::Map("guard address overflow"))?;
    let guard_end = guard_base
        .checked_add(SMALL_DATA_GUARD)
        .ok_or(InitError::Map("guard range overflow"))?;
    let handle = guard_vmo_handle()?;
    let guard_len = SMALL_DATA_GUARD;
    tracker.ensure_available(guard_base, guard_len, RangeKind::Guard)?;
    if let Err(err) = nexus_abi::as_map(as_handle, handle, guard_base, guard_len, PROT_READ, 0) {
        return Err(InitError::Abi(err));
    }
    tracker.insert(guard_base, guard_len, RangeKind::Guard);
    nexus_log::trace("init", |line| {
        line.text("guard base=");
        line.hex(guard_base);
        line.text(" end=");
        line.hex(guard_end);
    });
    Ok(())
}

fn map_stack(as_handle: nexus_abi::AsHandle, pages: u64) -> Result<(nexus_abi::Handle, u64)> {
    let adjusted_pages = pages.max(1);
    let size = adjusted_pages
        .checked_mul(PAGE_SIZE)
        .ok_or(InitError::Map("stack size overflow"))?;
    let base = USER_STACK_TOP
        .checked_sub(size)
        .ok_or(InitError::Map("stack base underflow"))?;
    let vmo = nexus_abi::vmo_create(size as usize).map_err(InitError::Ipc)?;
    if let Err(err) = nexus_abi::as_map(
        as_handle,
        vmo,
        base,
        size,
        PROT_READ | PROT_WRITE,
        MAP_FLAG_USER,
    ) {
        let _ = nexus_abi::vmo_destroy(vmo);
        return Err(InitError::Abi(err));
    }
    Ok((vmo, USER_STACK_TOP))
}

fn prot_from_flags(flags: u32) -> Result<u32> {
    const PF_X: u32 = 1;
    const PF_W: u32 = 2;
    const PF_R: u32 = 4;

    let exec = flags & PF_X != 0;
    let write = flags & PF_W != 0;
    let read = flags & PF_R != 0;

    if exec && write {
        return Err(InitError::Elf("wx segment not permitted"));
    }

    let mut prot = 0;
    if read {
        prot |= PROT_READ;
    }
    if write {
        prot |= PROT_WRITE;
    }
    if exec {
        prot |= PROT_EXEC;
    }
    Ok(prot)
}

fn align_up(value: u64, align: u64) -> Option<u64> {
    if align == 0 {
        return Some(value);
    }
    let mask = align - 1;
    value.checked_add(mask).map(|v| v & !mask)
}

impl From<AbiError> for InitError {
    fn from(err: AbiError) -> Self {
        Self::Abi(err)
    }
}

impl From<IpcError> for InitError {
    fn from(err: IpcError) -> Self {
        Self::Ipc(err)
    }
}

/// Helper that renders an [`InitError`] into the shared logging line format.
pub fn describe_init_error(line: &mut LineBuilder<'_, '_>, err: &InitError) {
    match err {
        InitError::Abi(code) => {
            let label = abi_error_label(*code);
            line.text("abi:");
            log_str_debug(line, label);
        }
        InitError::Ipc(code) => {
            let label = ipc_error_label(*code);
            line.text("ipc:");
            log_str_debug(line, label);
        }
        InitError::Elf(msg) => {
            line.text("elf:");
            log_str_debug(line, msg);
        }
        InitError::Map(msg) => {
            line.text("map:");
            log_str_debug(line, msg);
        }
        InitError::MissingElf => {
            line.text("missing-elf");
        }
    }
}

fn log_str_debug(line: &mut LineBuilder<'_, '_>, value: &str) {
    line.text(" ptr=0x");
    line.hex(value.as_ptr() as u64);
    line.text(" len=0x");
    line.hex(value.len() as u64);
    line.text(" text=");
    line.text_ref(StrRef::from(value));
}

fn log_image_bounds() {
    let ro_start = unsafe { &__rodata_start as *const u8 as usize };
    let ro_end = unsafe { &__rodata_end as *const u8 as usize };
    let guard = unsafe { &__small_data_guard as *const u8 as usize };
    let image_end = unsafe { &__image_end as *const u8 as usize };
    nexus_log::debug("init", |line| {
        line.text("image ro_start=0x");
        line.hex(ro_start as u64);
        line.text(" ro_end=0x");
        line.hex(ro_end as u64);
        line.text(" guard=0x");
        line.hex(guard as u64);
        line.text(" image_end=0x");
        line.hex(image_end as u64);
    });
}

fn abi_error_label(err: AbiError) -> &'static str {
    match err {
        AbiError::InvalidSyscall => "invalid-syscall",
        AbiError::CapabilityDenied => "capability-denied",
        AbiError::IpcFailure => "ipc-failure",
        AbiError::SpawnFailed => "spawn-failed",
        AbiError::TransferFailed => "transfer-failed",
        AbiError::ChildUnavailable => "child-unavailable",
        AbiError::NoSuchPid => "no-such-pid",
        AbiError::InvalidArgument => "invalid-argument",
        AbiError::Unsupported => "unsupported",
    }
}

fn ipc_error_label(err: IpcError) -> &'static str {
    match err {
        IpcError::NoSuchEndpoint => "no-such-endpoint",
        IpcError::QueueFull => "queue-full",
        IpcError::QueueEmpty => "queue-empty",
        IpcError::PermissionDenied => "permission-denied",
        IpcError::Unsupported => "unsupported",
    }
}
