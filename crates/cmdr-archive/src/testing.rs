//! Typed paths for this crate's tests, joined by the app path's own arithmetic,
//! so a test names an entry the way a pane does (under the archive's root) and
//! a Windows run never gains a `\`.

use cmdr_fs::app_path::{AppPath, AppPathBuf, RelativeAppPath};

/// `inner` (`/`-separated names, or `""` for `root` itself) under `root`.
#[track_caller]
pub(crate) fn at(root: &AppPath, inner: &str) -> AppPathBuf {
    if inner.is_empty() {
        return root.to_owned();
    }
    let relative = RelativeAppPath::new(inner).unwrap_or_else(|why| panic!("{inner:?} is no relative path: {why}"));
    root.join_relative(relative)
        .unwrap_or_else(|why| panic!("{inner:?} doesn't join under {root:?}: {why}"))
}
