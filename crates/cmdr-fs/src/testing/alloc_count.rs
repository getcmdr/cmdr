//! Counts heap allocations, for a test that pins an allocation ceiling.
//!
//! A binary gets exactly ONE global allocator, so the crate whose test binary measures
//! installs [`CountingAllocator`] itself:
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOCATOR: cmdr_fs::testing::alloc_count::CountingAllocator =
//!     cmdr_fs::testing::alloc_count::CountingAllocator;
//! ```
//!
//! Then [`allocations_during`] counts every allocation the whole process makes while its
//! closure runs, worker threads included (a listing may hop to a blocking pool). nextest
//! runs each test in its own process, so nothing else in the suite counts along. The
//! allocator is `System` with one relaxed load per call while no count is open.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

/// `System`, plus a counter of allocations (`alloc`, `alloc_zeroed`, `realloc`) that
/// [`allocations_during`] reads.
pub struct CountingAllocator;

fn count() {
    if COUNTING.load(Ordering::Relaxed) {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    }
}

// SAFETY: every method forwards its arguments unchanged to `System`, whose `GlobalAlloc`
// impl is sound; the only added work is two relaxed atomics, which never allocate and so
// can't re-enter the allocator.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: `layout` is our caller's, forwarded untouched.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: `layout` is our caller's, forwarded untouched.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count();
        // SAFETY: `ptr`, `layout`, and `new_size` are our caller's, forwarded untouched.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` and `layout` are our caller's, forwarded untouched.
        unsafe { System.dealloc(ptr, layout) }
    }
}

/// `work`'s result and how many allocations the process made while it ran. Reads zero
/// unless the test binary installed [`CountingAllocator`].
pub fn allocations_during<T>(work: impl FnOnce() -> T) -> (T, u64) {
    ALLOCATIONS.store(0, Ordering::SeqCst);
    COUNTING.store(true, Ordering::SeqCst);
    let out = work();
    COUNTING.store(false, Ordering::SeqCst);
    (out, ALLOCATIONS.load(Ordering::SeqCst))
}

/// The allocations one listed entry costs: a listing of one entry minus a listing of
/// none, so the fixed cost of a listing cancels out. `count_at(n)` lists a folder of `n`
/// entries and reports what that cost.
///
/// One entry, ❌ never a slope over many: a listing's sort can allocate per COMPARISON
/// (`InMemoryVolume` lowercases both names), and how many comparisons a sort makes
/// depends on the order a `HashMap` hands the entries over, which changes per process.
/// One entry makes no comparison, so the count is exact and the same every run.
pub fn allocations_for_one_entry(mut count_at: impl FnMut(u64) -> u64) -> u64 {
    // Warm whatever a first call sets up once (thread pools, lazy statics).
    count_at(1);
    let none = count_at(0);
    let one = count_at(1);
    one.saturating_sub(none)
}
