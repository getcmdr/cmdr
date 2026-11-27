//! The path cache forgets a folder's whole subtree however deep it runs (#1541).

use cmdr_fs::app_path::{AppPath, AppPathBuf};
use mtp_rs::ObjectHandle;

use super::cache::PathHandleCache;

/// `/d`, `/d/d`, … `depth` levels, each the next handle.
fn chain(depth: usize) -> (PathHandleCache, Vec<AppPathBuf>) {
    let mut cache = PathHandleCache::default();
    let mut paths = Vec::with_capacity(depth);
    let mut text = String::new();
    for handle in 1..=depth {
        text.push_str("/d");
        let path = AppPath::from_normalized(&text).expect("a canonical path").to_owned();
        cache.insert(path.clone(), ObjectHandle(handle as u64));
        paths.push(path);
    }
    (cache, paths)
}

/// Removing the top folder of a chain as deep as a phone's storage holds
/// (Android's 4,096-byte path limit is ~2,048 one-letter levels) forgets every
/// level on a small stack. The removal runs inside an MTP op, on top of its
/// futures on a 2 MiB tokio worker; one that recursed spent 256 B–1 KiB per
/// level in a debug build (this cell overflowed 512 KiB at 2,048 levels, a
/// fresh 2 MiB thread held it, macOS 27, 2026-11-27), so depth decided whether
/// the op's stack held.
#[test]
fn removing_a_folder_forgets_a_subtree_as_deep_as_a_phone_holds_on_a_small_stack() {
    const DEPTH: usize = 2_048;
    let (mut cache, paths) = chain(DEPTH);

    let cache = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(move || {
            cache.remove_path(&paths[0]);
            cache
        })
        .unwrap()
        .join()
        .expect("the removal ends without overflowing its stack");

    assert!(cache.path_to_handle.is_empty(), "every level's path went");
    assert!(cache.handle_to_path.is_empty(), "and every level's handle");
}
