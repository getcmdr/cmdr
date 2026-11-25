//! Windows capture of a native crash: an unhandled structured exception (access violation, stack
//! overflow, illegal instruction) and the C runtime's `abort()`.
//!
//! The Windows twin of `signal_handler.rs`, under the same rule: the crashing process may be holding
//! any lock and its heap may be the thing that's broken, so the crash path allocates nothing, takes
//! no lock, and formats nothing. It streams an `exception_record.rs` record into a file handle
//! opened at [`install`], and the next launch turns that into a `CrashReport`
//! (`next_launch::convert_raw_exception_crash`). Each call on the crash path is a syscall or a read
//! of our own stack; why each one is safe is at its site. `DETAILS.md` § Windows native crashes.

use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::os::windows::io::IntoRawHandle;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, AtomicUsize, Ordering};

use windows_sys::Wdk::Storage::FileSystem::NtQueryVirtualMemory;
use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE, UNICODE_STRING};
use windows_sys::Win32::Storage::FileSystem::{FILE_BEGIN, SetEndOfFile, SetFilePointerEx, WriteFile};
use windows_sys::Win32::System::Diagnostics::Debug::{
    EXCEPTION_CONTINUE_SEARCH, EXCEPTION_POINTERS, LPTOP_LEVEL_EXCEPTION_FILTER, RtlCaptureStackBackTrace,
    SetUnhandledExceptionFilter,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{MEM_IMAGE, MEMORY_BASIC_INFORMATION, VirtualQuery};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::GetCurrentProcess;

use super::exception_record::{self, Header, Kind, MAX_PARAMS, MODULE_NAME_MAX, Sink};

/// Frames walked on the crash path. A stack overflow's recursion fills them all, which is the part
/// worth seeing.
const MAX_FRAMES: usize = 64;
/// Distinct images named per record; a frame in a further one keeps its address only.
const MAX_MODULES: usize = 32;
/// `MemoryMappedFilenameInformation`, the `NtQueryVirtualMemory` class that names an image's file.
const MEMORY_MAPPED_FILENAME_INFORMATION: i32 = 2;
/// The CRT's `SIGABRT` and the `signal()` sentinels (`signal.h`).
const SIGABRT: i32 = 22;
const SIG_DFL: usize = 0;
const SIG_IGN: usize = 1;
const SIG_ERR: usize = usize::MAX;

unsafe extern "C" {
    /// The C runtime's `signal`. `abort()` raises `SIGABRT` through it before it ends the process.
    fn signal(sig: i32, handler: usize) -> usize;
}

/// The record's file, opened at [`install`]; `INVALID_HANDLE_VALUE` until then or when it wouldn't open.
static RAW_HANDLE: AtomicPtr<c_void> = AtomicPtr::new(INVALID_HANDLE_VALUE);
/// Our executable's load address, read at [`install`].
static IMAGE_BASE: AtomicU64 = AtomicU64::new(0);
/// `GetTickCount64` at [`install`], for the record's uptime.
static START_TICK: AtomicU64 = AtomicU64::new(0);
/// The filter that was in place before ours, as a `usize` (0 for none), so ours can hand on to it.
static PREVIOUS_FILTER: AtomicUsize = AtomicUsize::new(0);
/// The `SIGABRT` handler that was in place before ours, or `SIG_DFL`.
static PREVIOUS_ABORT: AtomicUsize = AtomicUsize::new(SIG_DFL);
/// Set by the first crash path in; a fault inside the capture, or a second thread crashing at the
/// same moment, writes nothing over the first record.
static RECORDING: AtomicBool = AtomicBool::new(false);

/// Opens the record file and installs the unhandled-exception filter and the `SIGABRT` handler.
/// Normal context: everything the crash path needs is resolved here.
pub(super) fn install(raw_crash_path: &Path) {
    // A new file private to this user (`cmdr_fs::private_file`), under this process's own name, so
    // no other instance shares it. The handle is ours for the process's life: the crash path writes
    // through it, the OS closes it when the process ends, and holding it open for writing is what
    // tells a second instance's next-launch pass that this record is live
    // (`raw_crash_file_in_use`). Not inheritable, so no child process holds it.
    match cmdr_fs::private_file::create_new(raw_crash_path) {
        Ok(file) => RAW_HANDLE.store(file.into_raw_handle(), Ordering::SeqCst),
        Err(e) => {
            log::warn!("Crash reporter: couldn't open the raw crash file, so a native crash won't be recorded: {e}")
        }
    }
    IMAGE_BASE.store(current_image_base(), Ordering::SeqCst);
    // SAFETY: reads the tick counter; no arguments.
    START_TICK.store(unsafe { GetTickCount64() }, Ordering::SeqCst);

    // SAFETY: `filter` is an `extern "system"` function with the filter signature, alive for the
    // process's life. The previous filter (or `None`) comes back and is kept for chaining.
    let previous = unsafe { SetUnhandledExceptionFilter(Some(filter)) };
    PREVIOUS_FILTER.store(previous.map_or(0, |f| f as usize), Ordering::SeqCst);

    // SAFETY: `on_abort` is an `extern "C" fn(i32)`, the handler shape `signal` takes, alive for the
    // process's life. `signal` returns the previous handler, or `SIG_ERR`.
    let previous_abort = unsafe { signal(SIGABRT, on_abort as extern "C" fn(i32) as usize) };
    if previous_abort != SIG_ERR {
        PREVIOUS_ABORT.store(previous_abort, Ordering::SeqCst);
    }
}

/// Our executable's load address, or 0. `GetModuleHandleW(NULL)` reads the process's own image base
/// and takes no loader lock, so it's fine in the panic hook too.
pub(super) fn current_image_base() -> u64 {
    // SAFETY: a null module name asks for the executable's own handle, which is its load address.
    unsafe { GetModuleHandleW(std::ptr::null()) as u64 }
}

/// The unhandled-exception filter: record, then hand on to whatever was there before, or let the
/// search go on so Windows Error Reporting ends the process as it would without us.
unsafe extern "system" fn filter(info: *const EXCEPTION_POINTERS) -> i32 {
    if !RECORDING.swap(true, Ordering::SeqCst) {
        // SAFETY: Windows passes the dying thread's exception pointers, valid for this call.
        unsafe { record_exception(info) };
    }
    let previous = PREVIOUS_FILTER.load(Ordering::SeqCst);
    if previous == 0 {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    // SAFETY: `previous` is the non-null filter `SetUnhandledExceptionFilter` handed back at install,
    // stored as a `usize`; transmuting it back gives the same function pointer.
    let previous: LPTOP_LEVEL_EXCEPTION_FILTER =
        unsafe { std::mem::transmute::<usize, LPTOP_LEVEL_EXCEPTION_FILTER>(previous) };
    match previous {
        // SAFETY: the previous filter, called with the pointers Windows gave us, as it would have been.
        Some(previous) => unsafe { previous(info) },
        None => EXCEPTION_CONTINUE_SEARCH,
    }
}

/// The CRT's `SIGABRT` handler: record, then hand on to the previous handler. Returning lets
/// `abort()` go on to end the process.
extern "C" fn on_abort(sig: i32) {
    if !RECORDING.swap(true, Ordering::SeqCst) {
        let header = Header {
            kind: Kind::Abort,
            code: 0,
            address: 0,
            param_count: 0,
            params: [0; MAX_PARAMS],
            image_base: IMAGE_BASE.load(Ordering::Relaxed),
            uptime_ms: uptime_ms(),
        };
        write_record(&header);
    }
    let previous = PREVIOUS_ABORT.load(Ordering::SeqCst);
    if previous != SIG_DFL && previous != SIG_IGN {
        // SAFETY: `previous` is the handler `signal` handed back at install, a C `void(int)`.
        let previous = unsafe { std::mem::transmute::<usize, extern "C" fn(i32)>(previous) };
        previous(sig);
    }
}

/// # Safety
/// `info` must be the exception pointers Windows passed the filter.
unsafe fn record_exception(info: *const EXCEPTION_POINTERS) {
    // SAFETY: the caller's contract; a null record pointer is checked before use.
    let record = unsafe { (*info).ExceptionRecord };
    if record.is_null() {
        return;
    }
    // SAFETY: non-null, and Windows keeps it valid for the filter call.
    let record = unsafe { &*record };
    let mut params = [0u64; MAX_PARAMS];
    for (param, value) in params.iter_mut().zip(record.ExceptionInformation.iter()) {
        *param = *value as u64;
    }
    let header = Header {
        kind: Kind::Exception,
        code: record.ExceptionCode as u32,
        address: record.ExceptionAddress as u64,
        param_count: record.NumberParameters,
        params,
        image_base: IMAGE_BASE.load(Ordering::Relaxed),
        uptime_ms: uptime_ms(),
    };
    write_record(&header);
}

/// The largest record: the header, every module named at full length, every frame, plus the fault
/// address when the walk never passed it.
const RECORD_CAPACITY: usize =
    exception_record::HEADER_LEN + MAX_MODULES * (11 + MODULE_NAME_MAX * 2) + (MAX_FRAMES + 1) * 17;

/// Where the record is assembled before its one write. A static, not a stack array, so a stack
/// overflow's thin reserve doesn't also have to hold ~6 KB of record.
struct RecordBuffer(UnsafeCell<[u8; RECORD_CAPACITY]>);

// SAFETY: only the one thread that won `RECORDING` ever touches the buffer, once per process.
unsafe impl Sync for RecordBuffer {}

static RECORD_BUFFER: RecordBuffer = RecordBuffer(UnsafeCell::new([0; RECORD_CAPACITY]));

/// Assembles the header, then the stack, in [`RECORD_BUFFER`], and writes it to the file in one go.
/// Call only after winning [`RECORDING`].
fn write_record(header: &Header) {
    let handle = RAW_HANDLE.load(Ordering::SeqCst);
    if handle == INVALID_HANDLE_VALUE {
        return;
    }
    // SAFETY: the caller won `RECORDING`, so no other thread holds this borrow, ever.
    let buffer = unsafe { &mut *RECORD_BUFFER.0.get() };
    let mut sink = BufferSink {
        buffer,
        len: 0,
        full: false,
    };
    exception_record::write_header(&mut sink, header, env!("CARGO_PKG_VERSION"));
    let fault = (header.kind == Kind::Exception).then_some(header.address);
    walk_stack(fault, |event| match event {
        StackEvent::Module { base, name } => exception_record::write_module(&mut sink, base, name),
        StackEvent::Frame { address, module_base } => exception_record::write_frame(&mut sink, address, module_base),
    });
    // SAFETY: `handle` is the file we opened at install; seeking to 0 and cutting the file there
    // are plain syscalls.
    unsafe {
        SetFilePointerEx(handle, 0, std::ptr::null_mut(), FILE_BEGIN);
        SetEndOfFile(handle);
    }
    write_all(handle, &sink.buffer[..sink.len]);
}

/// Collects record pieces in a fixed buffer. A piece that doesn't fit ends the record there, and
/// every later piece is dropped too, so the file never holds a record with a hole in it (the
/// reader stops at a cut, but would misread a later piece as the cut one's continuation).
struct BufferSink<'a> {
    buffer: &'a mut [u8; RECORD_CAPACITY],
    len: usize,
    full: bool,
}

impl Sink for BufferSink<'_> {
    fn put(&mut self, bytes: &[u8]) {
        if self.full || self.len + bytes.len() > RECORD_CAPACITY {
            self.full = true;
            return;
        }
        self.buffer[self.len..self.len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();
    }
}

fn uptime_ms() -> u64 {
    // SAFETY: reads the tick counter; no arguments.
    unsafe { GetTickCount64() }.saturating_sub(START_TICK.load(Ordering::Relaxed))
}

/// What [`walk_stack`] reports, in order: a module the first time a frame lands in it, then the frame.
enum StackEvent<'a> {
    Module { base: u64, name: &'a [u16] },
    Frame { address: u64, module_base: u64 },
}

/// Walks this thread's stack, innermost first, starting at `fault` when the walk passes it (the
/// exception filter's own frames and Windows' dispatcher sit above it), and names each image a
/// frame lies in. Allocation-free: its buffers are on the stack, under 2 KB in all, which fits the
/// stack overflow's reserve on a thread Rust's runtime made (20 KB). A thread the OS made for
/// someone else (COM, the thread pool, an injected DLL) has no such reserve: `DETAILS.md`.
fn walk_stack(fault: Option<u64>, mut on: impl FnMut(StackEvent<'_>)) {
    let mut frames = [std::ptr::null_mut::<c_void>(); MAX_FRAMES];
    // SAFETY: `frames` holds exactly `MAX_FRAMES` slots and we ask for that many; the hash
    // out-parameter is optional. Walks this thread's own stack through the unwind data of the
    // images on it.
    let count = unsafe { RtlCaptureStackBackTrace(0, MAX_FRAMES as u32, frames.as_mut_ptr(), std::ptr::null_mut()) };
    let frames = &frames[..usize::from(count).min(MAX_FRAMES)];
    let start = fault.and_then(|fault| frames.iter().position(|&frame| frame as u64 == fault));

    let mut named = [0u64; MAX_MODULES];
    let mut named_count = 0;
    let mut name = NameBuffer::new();
    let mut emit = |address: u64, on: &mut dyn FnMut(StackEvent<'_>)| {
        let module_base = image_base_of(address);
        if module_base != 0 && !named[..named_count].contains(&module_base) && named_count < MAX_MODULES {
            named[named_count] = module_base;
            named_count += 1;
            // allowed-discarded-outcome: `on` is a `()` sink for the walk's events; there's nothing to drop.
            on(StackEvent::Module {
                base: module_base,
                name: name.read(module_base),
            });
        }
        // allowed-discarded-outcome: as above.
        on(StackEvent::Frame { address, module_base });
    };
    match (fault, start) {
        (_, Some(start)) => {
            for &frame in &frames[start..] {
                emit(frame as u64, &mut on);
            }
        }
        // The walk never passed the fault (a corrupt stack, say): lead with it, then everything.
        (Some(fault), None) => {
            emit(fault, &mut on);
            for &frame in frames {
                emit(frame as u64, &mut on);
            }
        }
        (None, None) => {
            for &frame in frames {
                emit(frame as u64, &mut on);
            }
        }
    }
}

/// The load address of the image holding `address`, or 0 when no image does (the stack, the heap,
/// JIT code). `VirtualQuery` is one syscall into the memory manager; it takes no user-mode lock.
fn image_base_of(address: u64) -> u64 {
    let mut info = MEMORY_BASIC_INFORMATION::default();
    // SAFETY: `info` is a stack local of the size we pass; the call only writes it, and any
    // address is a valid query (an unmapped one fails or reports free memory).
    let written = unsafe {
        VirtualQuery(
            address as *const c_void,
            &raw mut info,
            size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    if written == 0 || info.Type != MEM_IMAGE {
        return 0;
    }
    info.AllocationBase as u64
}

/// Room for `NtQueryVirtualMemory`'s answer: a `UNICODE_STRING` followed by the name it points at.
/// A device path longer than this (500-odd characters) leaves the module unnamed.
#[repr(C, align(8))]
struct NameBuffer([u8; 1040]);

impl NameBuffer {
    fn new() -> Self {
        Self([0; 1040])
    }

    /// The file path of the image at `base`, as the kernel spells it (`\Device\HarddiskVolume3\…`),
    /// or empty. The record writer keeps only the part after the last backslash.
    fn read(&mut self, base: u64) -> &[u16] {
        let mut returned = 0usize;
        // SAFETY: `self.0` is 8-aligned and `self.0.len()` bytes long, which we pass; the kernel
        // writes a `UNICODE_STRING` at its start whose buffer points into the rest of it. A syscall,
        // with no user-mode lock or allocation (unlike `GetMappedFileNameW`'s wrappers).
        let status = unsafe {
            NtQueryVirtualMemory(
                GetCurrentProcess(),
                base as *const c_void,
                MEMORY_MAPPED_FILENAME_INFORMATION,
                self.0.as_mut_ptr().cast(),
                self.0.len(),
                &raw mut returned,
            )
        };
        if status < 0 {
            return &[];
        }
        // SAFETY: on success the buffer starts with a `UNICODE_STRING`, and it's 8-aligned.
        let header = unsafe { &*self.0.as_ptr().cast::<UNICODE_STRING>() };
        let units = usize::from(header.Length) / 2;
        let start = self.0.as_ptr() as usize;
        let text = header.Buffer as usize;
        // Only a name that lies wholly inside our own buffer, which is where the kernel puts it.
        let fits = text >= start + size_of::<UNICODE_STRING>() && text + units * 2 <= start + self.0.len();
        if header.Buffer.is_null() || !fits {
            return &[];
        }
        // SAFETY: checked just above to lie inside `self.0`, which outlives the returned borrow;
        // the kernel wrote `units` UTF-16 units there.
        unsafe { std::slice::from_raw_parts(header.Buffer, units) }
    }
}

/// Writes `bytes` to the record's file handle. `WriteFile` on a synchronous handle is a syscall,
/// and a whole record takes one call; a short or failed write is dropped, since nothing on the
/// crash path can do better.
fn write_all(handle: HANDLE, bytes: &[u8]) {
    let mut written_total = 0usize;
    while written_total < bytes.len() {
        let rest = &bytes[written_total..];
        let mut written = 0u32;
        // SAFETY: `rest` is valid for `rest.len()` bytes (at most `RECORD_CAPACITY`); the handle is
        // the synchronous file we opened, so no `OVERLAPPED` is needed.
        let ok = unsafe {
            WriteFile(
                handle,
                rest.as_ptr(),
                rest.len() as u32,
                &raw mut written,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 || written == 0 {
            return;
        }
        written_total += written as usize;
    }
}

/// This thread's stack as `module+0xoffset` frames, for the panic hook. Normal context (it
/// allocates), but the same walk the crash path makes.
pub(super) fn module_frames() -> Vec<String> {
    let mut modules: Vec<(u64, String)> = Vec::new();
    let mut frames = Vec::new();
    walk_stack(None, |event| match event {
        StackEvent::Module { base, name } => {
            let name = exception_record::file_name_of(name);
            let name = &name[..name.len().min(MODULE_NAME_MAX)];
            modules.push((base, String::from_utf16_lossy(name)));
        }
        StackEvent::Frame { address, module_base } => {
            let module = modules
                .iter()
                .find(|(base, _)| *base == module_base)
                .map(|(_, name)| name.as_str());
            frames.push(exception_record::render_frame(module, address, module_base));
        }
    });
    frames
}
