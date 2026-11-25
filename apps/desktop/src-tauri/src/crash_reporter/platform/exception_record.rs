//! The Windows native-crash record: the bytes `exception_filter.rs` writes mid-crash, and what the
//! next launch reads back out of them.
//!
//! The write side is generic over [`Sink`] and allocation-free, so the crashing process can stream
//! it straight into a pre-opened file handle; tests stream it into a `Vec`. Pure, so it's compiled
//! (and tested) on every host, though only Windows writes one. `DETAILS.md` § Windows native crashes.
//!
//! Format (little-endian), version 1:
//!   - header, [`HEADER_LEN`] bytes: magic `CMCW`, version u32, kind u8 + 3 pad, exception code u32,
//!     exception address u64, parameter count u32 + 4 pad, the first [`MAX_PARAMS`] exception
//!     parameters (u64 each), the main image's load address u64, uptime in ms u64, the app version
//!     (zero-padded ASCII, [`APP_VERSION_FIELD_LEN`] bytes)
//!   - then records, until the end of the file: tag `1` is a module (load address u64, name length
//!     u16 in UTF-16 units, the name), tag `2` a frame (address u64, its module's load address u64,
//!     0 when no image holds it). A record cut short by a dying process ends the list; everything
//!     before it still counts.
//!
//! A module's name is its FILE NAME only (`ntdll.dll`, `cmdr.exe`): the writer cuts everything up to
//! the last backslash before the name reaches the disk, so no path, and no user name inside one,
//! is ever written.

pub(in crate::crash_reporter) const MAGIC: &[u8; 4] = b"CMCW";
pub(in crate::crash_reporter) const VERSION: u32 = 1;
pub(in crate::crash_reporter) const APP_VERSION_FIELD_LEN: usize = 32;
/// How many of an exception's parameters we keep. An access violation uses two (read/write/execute,
/// then the address), an in-page error three (plus the NTSTATUS of the failed read).
pub(in crate::crash_reporter) const MAX_PARAMS: usize = 3;
/// A module name's cap, in UTF-16 units. Real image names are far shorter.
pub(in crate::crash_reporter) const MODULE_NAME_MAX: usize = 64;
pub(in crate::crash_reporter) const HEADER_LEN: usize =
    4 + 4 + 4 + 4 + 8 + 4 + 4 + MAX_PARAMS * 8 + 8 + 8 + APP_VERSION_FIELD_LEN;
const TAG_MODULE: u8 = 1;
const TAG_FRAME: u8 = 2;

/// What ended the process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::crash_reporter) enum Kind {
    /// A structured exception nothing handled: an access violation, a stack overflow, an illegal
    /// instruction.
    Exception,
    /// The C runtime's `abort()`, seen through its `SIGABRT` handler.
    Abort,
}

impl Kind {
    fn to_byte(self) -> u8 {
        match self {
            Self::Exception => 0,
            Self::Abort => 1,
        }
    }

    fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::Exception),
            1 => Some(Self::Abort),
            _ => None,
        }
    }
}

/// The fixed part of a record: everything known before the stack walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::crash_reporter) struct Header {
    pub kind: Kind,
    pub code: u32,
    pub address: u64,
    pub param_count: u32,
    pub params: [u64; MAX_PARAMS],
    pub image_base: u64,
    pub uptime_ms: u64,
}

/// Where the record goes. ❗ An implementation used by the crash path may not allocate or lock.
pub(in crate::crash_reporter) trait Sink {
    fn put(&mut self, bytes: &[u8]);
}

#[cfg(test)]
impl Sink for Vec<u8> {
    fn put(&mut self, bytes: &[u8]) {
        self.extend_from_slice(bytes);
    }
}

pub(in crate::crash_reporter) fn write_header(sink: &mut impl Sink, header: &Header, app_version: &str) {
    sink.put(MAGIC);
    sink.put(&VERSION.to_le_bytes());
    sink.put(&[header.kind.to_byte(), 0, 0, 0]);
    sink.put(&header.code.to_le_bytes());
    sink.put(&header.address.to_le_bytes());
    sink.put(&header.param_count.to_le_bytes());
    sink.put(&[0; 4]);
    for param in header.params {
        sink.put(&param.to_le_bytes());
    }
    sink.put(&header.image_base.to_le_bytes());
    sink.put(&header.uptime_ms.to_le_bytes());
    let mut version = [0u8; APP_VERSION_FIELD_LEN];
    let len = app_version.len().min(APP_VERSION_FIELD_LEN);
    version[..len].copy_from_slice(&app_version.as_bytes()[..len]);
    sink.put(&version);
}

/// A module record. `name` is cut to the part after its last backslash and capped at
/// [`MODULE_NAME_MAX`] here, so a caller holding a full path can't put it on disk.
pub(in crate::crash_reporter) fn write_module(sink: &mut impl Sink, base: u64, name: &[u16]) {
    let name = file_name_of(name);
    let name = &name[..name.len().min(MODULE_NAME_MAX)];
    sink.put(&[TAG_MODULE]);
    sink.put(&base.to_le_bytes());
    // At most `MODULE_NAME_MAX` (64), so the cast can't truncate.
    sink.put(&(name.len() as u16).to_le_bytes());
    for unit in name {
        sink.put(&unit.to_le_bytes());
    }
}

pub(in crate::crash_reporter) fn write_frame(sink: &mut impl Sink, address: u64, module_base: u64) {
    sink.put(&[TAG_FRAME]);
    sink.put(&address.to_le_bytes());
    sink.put(&module_base.to_le_bytes());
}

/// The units after the last `\` or `/`.
pub(in crate::crash_reporter) fn file_name_of(name: &[u16]) -> &[u16] {
    match name
        .iter()
        .rposition(|&unit| unit == u16::from(b'\\') || unit == u16::from(b'/'))
    {
        Some(separator) => &name[separator + 1..],
        None => name,
    }
}

/// One frame as a report shows it: `cmdr.exe+0x1a2b3c` when an image holds it, else the bare
/// address. The module-relative offset is stable across launches and machines (ASLR moves only the
/// base), so it groups and resolves against that build's PDB, or Microsoft's symbol server for a
/// Windows DLL.
pub(in crate::crash_reporter) fn render_frame(module: Option<&str>, address: u64, module_base: u64) -> String {
    match module {
        Some(name) if module_base != 0 && address >= module_base => {
            let name = if name.is_empty() { "unknown-module" } else { name };
            format!("{name}+0x{:x}", address - module_base)
        }
        _ => format!("0x{address:016x}"),
    }
}

/// A record read back at the next launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::crash_reporter) struct ExceptionRecord {
    pub header: Header,
    pub app_version: String,
    /// Load address and file name of every image a frame lies in.
    pub modules: Vec<(u64, String)>,
    /// Address and its module's load address (0 for none), innermost first.
    pub frames: Vec<(u64, u64)>,
}

/// Parses a record, or `None` when it isn't one (empty, cut inside the header, another format or
/// version).
pub(in crate::crash_reporter) fn parse(data: &[u8]) -> Option<ExceptionRecord> {
    if data.len() < HEADER_LEN || &data[0..4] != MAGIC || u32_at(data, 4)? != VERSION {
        return None;
    }
    let kind = Kind::from_byte(data[8])?;
    let mut params = [0u64; MAX_PARAMS];
    for (i, param) in params.iter_mut().enumerate() {
        *param = u64_at(data, 32 + i * 8)?;
    }
    let version_start = 32 + MAX_PARAMS * 8 + 16;
    let header = Header {
        kind,
        code: u32_at(data, 12)?,
        address: u64_at(data, 16)?,
        param_count: u32_at(data, 24)?,
        params,
        image_base: u64_at(data, version_start - 16)?,
        uptime_ms: u64_at(data, version_start - 8)?,
    };
    let app_version = std::str::from_utf8(&data[version_start..HEADER_LEN])
        .ok()?
        .trim_end_matches('\0')
        .to_string();

    let mut record = ExceptionRecord {
        header,
        app_version,
        modules: Vec::new(),
        frames: Vec::new(),
    };
    let mut at = HEADER_LEN;
    while at < data.len() {
        match data[at] {
            TAG_MODULE => {
                let Some(base) = u64_at(data, at + 1) else { break };
                let Some(len) = u16_at(data, at + 9) else { break };
                let start = at + 11;
                let end = start + usize::from(len) * 2;
                let Some(bytes) = data.get(start..end) else { break };
                let units: Vec<u16> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .collect();
                // Cut again on read: a record from a writer that didn't is still never a path.
                record
                    .modules
                    .push((base, String::from_utf16_lossy(file_name_of(&units))));
                at = end;
            }
            TAG_FRAME => {
                let (Some(address), Some(module_base)) = (u64_at(data, at + 1), u64_at(data, at + 9)) else {
                    break;
                };
                record.frames.push((address, module_base));
                at += 17;
            }
            _ => break,
        }
    }
    Some(record)
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(data: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(data.get(at..at + 8)?.try_into().ok()?))
}

const ACCESS_VIOLATION: u32 = 0xC000_0005;
const IN_PAGE_ERROR: u32 = 0xC000_0006;

/// The `EXCEPTION_*` name Windows' headers give a code, for the codes a crash can carry.
pub(in crate::crash_reporter) fn exception_name(code: u32) -> Option<&'static str> {
    Some(match code {
        ACCESS_VIOLATION => "EXCEPTION_ACCESS_VIOLATION",
        IN_PAGE_ERROR => "EXCEPTION_IN_PAGE_ERROR",
        0xC000_00FD => "EXCEPTION_STACK_OVERFLOW",
        0xC000_001D => "EXCEPTION_ILLEGAL_INSTRUCTION",
        0xC000_0096 => "EXCEPTION_PRIV_INSTRUCTION",
        0xC000_0094 => "EXCEPTION_INT_DIVIDE_BY_ZERO",
        0xC000_0095 => "EXCEPTION_INT_OVERFLOW",
        0xC000_008C => "EXCEPTION_ARRAY_BOUNDS_EXCEEDED",
        0xC000_0025 => "EXCEPTION_NONCONTINUABLE_EXCEPTION",
        0xC000_0026 => "EXCEPTION_INVALID_DISPOSITION",
        0xC000_0008 => "EXCEPTION_INVALID_HANDLE",
        0x8000_0002 => "EXCEPTION_DATATYPE_MISALIGNMENT",
        0x8000_0003 => "EXCEPTION_BREAKPOINT",
        0x8000_0004 => "EXCEPTION_SINGLE_STEP",
        0x8000_0001 => "EXCEPTION_GUARD_PAGE",
        // What MSVC's `throw` raises: a C++ exception (in WebView2's code, say) nothing caught.
        0xE06D_7363 => "MSVC_CPP_EXCEPTION",
        _ => return None,
    })
}

impl ExceptionRecord {
    /// The report's `signal`: the exception's name (`EXCEPTION_ACCESS_VIOLATION`), `SIGABRT` for an
    /// `abort()`, or `EXCEPTION 0x…` for a code without a name.
    pub(in crate::crash_reporter) fn signal(&self) -> String {
        match self.header.kind {
            Kind::Abort => "SIGABRT".to_string(),
            Kind::Exception => exception_name(self.header.code)
                .map_or_else(|| format!("EXCEPTION 0x{:08X}", self.header.code), str::to_string),
        }
    }

    /// The report's `osException`: what Windows' exception record says, in one line, like
    /// `EXCEPTION_ACCESS_VIOLATION (0xC0000005): reading 0x0000000000000010, at cmdr.exe+0x1a2b`.
    pub(in crate::crash_reporter) fn exception_line(&self) -> String {
        let header = &self.header;
        if header.kind == Kind::Abort {
            return "abort() called".to_string();
        }
        let mut line = format!("{} (0x{:08X})", self.signal(), header.code);
        if matches!(header.code, ACCESS_VIOLATION | IN_PAGE_ERROR) && header.param_count >= 2 {
            let operation = match header.params[0] {
                0 => "reading",
                1 => "writing",
                8 => "executing",
                _ => "accessing",
            };
            line.push_str(&format!(": {operation} 0x{:016x}", header.params[1]));
            if header.code == IN_PAGE_ERROR && header.param_count >= 3 {
                line.push_str(&format!(" (status 0x{:08X})", header.params[2]));
            }
        }
        line.push_str(", at ");
        line.push_str(&self.locate(header.address));
        line
    }

    /// The faulting thread's stack as `module+0xoffset` strings, innermost first: the report's
    /// `osFrames`.
    pub(in crate::crash_reporter) fn module_frames(&self) -> Vec<String> {
        self.frames
            .iter()
            .map(|&(address, module_base)| render_frame(self.module_name(module_base), address, module_base))
            .collect()
    }

    /// The same frames as bare addresses: the report's `backtraceFrames`, as on the macOS signal path.
    pub(in crate::crash_reporter) fn raw_frames(&self) -> Vec<String> {
        self.frames
            .iter()
            .map(|&(address, _)| format!("0x{address:016x}"))
            .collect()
    }

    fn module_name(&self, base: u64) -> Option<&str> {
        self.modules
            .iter()
            .find(|(module_base, _)| *module_base == base && base != 0)
            .map(|(_, name)| name.as_str())
    }

    /// The exception address in `module+0xoffset` form when a frame placed it, else bare.
    fn locate(&self, address: u64) -> String {
        match self.frames.iter().find(|(frame, _)| *frame == address) {
            Some(&(_, module_base)) => render_frame(self.module_name(module_base), address, module_base),
            None => format!("0x{address:016x}"),
        }
    }
}
