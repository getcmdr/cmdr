//! Linux: the region comes from `/proc/self/maps`, read with `open` / `read` / `close`.
//!
//! A read past the end of a mapped file's backing store (truncated, or on a filesystem that
//! answers `EIO` because its device went away) raises SIGBUS with `BUS_ADRERR`. Linux has no
//! add-image callback, so an image is recognized from the same maps read: a file also mapped
//! executable is a loaded ELF object, and none of its pages is recovered.

use std::sync::Once;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::super::proc_maps::{Lines, MapsLine, parse};
use super::verdict::{Region, page_to_recover};

#[cfg(any(test, feature = "testing"))]
pub(crate) use super::unix::install_test_handler;

static PAGE_SIZE: AtomicU64 = AtomicU64::new(0);
static READY: AtomicBool = AtomicBool::new(false);

pub(crate) fn prepare() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: `_SC_PAGESIZE` is a valid `sysconf` name with no side effects.
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        PAGE_SIZE.store(if page_size > 0 { page_size as u64 } else { 0 }, Ordering::SeqCst);
        READY.store(true, Ordering::SeqCst);
    });
}

/// Runs `on_line` over every line of `/proc/self/maps` until it answers `false`. Answers whether
/// the file could be read at all.
fn for_each_maps_line(mut on_line: impl FnMut(&[u8]) -> bool) -> bool {
    // SAFETY: a NUL-terminated literal path; `open` is async-signal-safe.
    let fd = unsafe { libc::open(c"/proc/self/maps".as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
    if fd < 0 {
        return false;
    }
    let mut lines = Lines::new();
    let mut chunk = [0u8; 4096];
    loop {
        // SAFETY: `chunk` is a live stack buffer of exactly the length passed; `read` is
        // async-signal-safe.
        let read = unsafe { libc::read(fd, chunk.as_mut_ptr().cast(), chunk.len()) };
        if read <= 0 {
            lines.finish(&mut on_line);
            break;
        }
        if !lines.feed(&chunk[..read as usize], &mut on_line) {
            break;
        }
    }
    // SAFETY: `fd` is the descriptor opened above; `close` is async-signal-safe.
    unsafe { libc::close(fd) };
    true
}

/// The mapping covering `address`, if any.
fn mapping_at(address: u64) -> Option<MapsLine> {
    let mut found = None;
    for_each_maps_line(|line| match parse(line) {
        Some(mapping) if mapping.start <= address && address < mapping.end => {
            found = Some(mapping);
            false
        }
        _ => true,
    });
    found
}

/// Whether the file behind `mapping` is also mapped executable, which makes it a loaded image.
/// An unreadable maps file counts as yes.
fn is_image(mapping: &MapsLine) -> bool {
    let mut image = false;
    let read = for_each_maps_line(|line| {
        if let Some(other) = parse(line)
            && other.executable
            && other.same_file(mapping)
        {
            image = true;
            return false;
        }
        true
    });
    image || !read
}

/// Async-signal-safe: two passes over `/proc/self/maps` on the stack, one `mmap`.
pub(crate) fn recover_page(info: &libc::siginfo_t) -> bool {
    if !READY.load(Ordering::SeqCst) || info.si_code != libc::BUS_ADRERR {
        return false;
    }
    // SAFETY: a SIGBUS `siginfo_t` carries the fault address.
    let address = unsafe { info.si_addr() } as u64;
    let Some(mapping) = mapping_at(address) else {
        return false;
    };
    if !mapping.readable {
        return false;
    }
    let region = Region {
        start: mapping.start,
        end: mapping.end,
        file_backed: mapping.is_file_backed(),
        writable: mapping.writable,
        executable: mapping.executable,
    };
    let in_image = mapping.is_file_backed() && is_image(&mapping);
    let page_size = PAGE_SIZE.load(Ordering::SeqCst);
    let Some(page) = page_to_recover(address, page_size, Some(region), in_image) else {
        return false;
    };
    super::unix::map_zeros_over(page, page_size)
}
