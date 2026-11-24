//! The dedicated threads a real place's icon lookup runs on, shared by the macOS and Windows
//! arms. Both OS lookups can descend into code we don't own (macOS: a FileProvider XPC chain on
//! a cloud folder; Windows: a third-party icon handler or a cloud filter), deep enough to
//! overflow rayon's 2 MB worker stack.

use super::PathIcons;

/// 8 MB stack per thread: enough for the deep FileProvider XPC chains NSWorkspace's per-path
/// lookup descends into on cloud folders, and for any Shell icon handler.
const ICON_THREAD_STACK_SIZE: usize = 8 * 1024 * 1024;

/// Runs `fetch` for each of `paths` on up to one 8 MB-stack thread per core, keyed
/// `path:{path}`, in input order.
pub(super) fn fetch_on_icon_threads(paths: Vec<String>, fetch: impl Fn(&str) -> Option<String> + Sync) -> PathIcons {
    if paths.is_empty() {
        return Vec::new();
    }
    let num_threads = paths
        .len()
        .min(std::thread::available_parallelism().map_or(4, |n| n.get()));
    let fetch = &fetch;

    std::thread::scope(|scope| {
        let chunk_size = paths.len().div_ceil(num_threads);
        let handles: Vec<_> = paths
            .chunks(chunk_size)
            .map(|chunk| {
                std::thread::Builder::new()
                    .stack_size(ICON_THREAD_STACK_SIZE)
                    .name("icon_path_fetch".into())
                    .spawn_scoped(scope, move || {
                        chunk
                            .iter()
                            .map(|path| (format!("path:{path}"), fetch(path)))
                            .collect::<Vec<_>>()
                    })
                    .expect("failed to spawn icon path-fetch thread")
            })
            .collect();

        let mut results = Vec::with_capacity(paths.len());
        for handle in handles {
            results.extend(handle.join().expect("icon path-fetch thread panicked"));
        }
        results
    })
}
