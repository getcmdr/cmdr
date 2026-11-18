//! The app path's allocation budget, counted by this binary's own global allocator: what a
//! listing pays per entry once it builds `FileEntry.path` with `AppPath::join`, against the
//! `PathBuf::join` + `to_string_lossy().into_owned()` it builds today (three, measured).
//!
//! Its own test binary because a binary gets exactly ONE global allocator: the lib's unit
//! tests run on the system one. Counts are per thread, so a parallel test can't leak into
//! another's measurement.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use cmdr_fs::app_path::{AppName, AppPath, AppPathBuf, PathFlavor};

thread_local! {
    // `const`-initialised and `Drop`-free: reading it never allocates, so the allocator below
    // can't re-enter itself.
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
}

struct Counting;

// SAFETY: every method forwards its arguments unchanged to `System`, whose `GlobalAlloc` impl
// is sound; the only added work is a `Drop`-free thread-local counter, which never allocates.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.with(|count| count.set(count.get() + 1));
        // SAFETY: `layout` is forwarded untouched from our caller, who upholds `alloc`'s contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.with(|count| count.set(count.get() + 1));
        // SAFETY: forwarded untouched; the block came from this allocator, which is `System`.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded untouched; the block came from this allocator, which is `System`.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// How many allocations (and reallocations) `work` makes on this thread.
fn allocations<T>(work: impl FnOnce() -> T) -> (u64, T) {
    let before = ALLOCATIONS.with(Cell::get);
    let out = work();
    (ALLOCATIONS.with(Cell::get) - before, out)
}

const FOLDER: &str = "/Users/test_user_1/Documents/Projects/cmdr/apps/desktop/src";
const NAME: &str = "a-file-with-a-typical-name.ts";

#[test]
fn a_listed_entry_costs_one_allocation_where_it_cost_three() {
    let folder = AppPath::from_normalized(FOLDER).expect("canonical");
    let name = AppName::new(NAME).expect("a plain name");
    let (typed, path) = allocations(|| folder.join(name).expect("joins"));
    assert_eq!(typed, 1, "AppPath::join: one String, sized exactly");
    assert_eq!(path.as_str().len(), FOLDER.len() + 1 + NAME.len());

    let std_folder = Path::new(FOLDER);
    let (today, wire) = allocations(|| std_folder.join(NAME).to_string_lossy().into_owned());
    // `PathBuf::join` copies the base, then grows it for the name (an allocation and a
    // reallocation), and `to_string_lossy().into_owned()` copies once more.
    assert_eq!(
        today, 3,
        "today's row: PathBuf::join (copy, then grow), then the String"
    );
    // On Windows today's join writes a `\` into the app path (#1225), the bug the type removes.
    if std::path::MAIN_SEPARATOR == '/' {
        assert_eq!(wire, path.as_str());
    }
}

#[test]
fn the_arithmetic_borrows() {
    let path = AppPath::from_normalized("/Users/test_user_1/Documents/notes.txt").expect("canonical");
    let other = AppPath::from_normalized("/Users/test_user_1/Documents/other.txt").expect("canonical");
    let base = AppPath::from_normalized("/Users/test_user_1").expect("canonical");
    let (count, ()) = allocations(|| {
        assert!(AppPath::from_normalized(path.as_str()).is_ok());
        assert_eq!(path.parent().and_then(AppPath::file_name), Some("Documents"));
        assert_eq!(path.file_name(), Some("notes.txt"));
        assert_eq!(path.names().count(), 4);
        assert!(path.starts_with(base));
        assert_eq!(
            path.strip_prefix(base).map(|rel| rel.as_str()),
            Some("Documents/notes.txt")
        );
        assert!(path.same_root(other));
        assert!(path != other && path < other);
        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        assert_ne!(hasher.finish(), 0);
        assert_eq!(path.shown(), path.as_str());
        assert_eq!(path.to_native_posix().map(|n| n.len()), Ok(path.as_str().len()));
    });
    assert_eq!(
        count, 0,
        "parent, file_name, names, starts_with, strip_prefix, Eq, Ord, Hash, shown, to_native_posix"
    );
}

#[test]
fn the_bridge_borrows_valid_text() {
    let std_path = PathBuf::from("/Users/test_user_1/a/");
    let (count, ()) = allocations(|| {
        let bridged = AppPath::legacy_unchecked(&std_path);
        assert!(matches!(bridged, std::borrow::Cow::Borrowed(_)));
        assert!(matches!(bridged.as_std_legacy(), std::borrow::Cow::Borrowed(_)));
    });
    assert_eq!(count, 0, "a UTF-8 path crosses the bridge both ways without a copy");
}

#[test]
fn parsing_builds_one_string() {
    let (count, path) =
        allocations(|| AppPathBuf::parse("/Users//test_user_1/./a/", PathFlavor::Posix).expect("parses"));
    assert_eq!(count, 1, "parse: one String for the normalized text");
    assert_eq!(path.as_str(), "/Users/test_user_1/a");
}
