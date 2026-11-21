//! Path arithmetic for indexing. Pure, lock-free helpers that map between the
//! filesystem's absolute paths and each volume's index path space.
//!
//! - [`routing`]: path -> owning-volume resolution (`volume_id_for_local_path`)
//!   and `index_read_path` (the read-side mount/scheme strip). Asked by volume id.
//! - [`path_space`]: `IndexPathSpace`, the seam that teaches the local scan /
//!   reconcile / live pipeline a mount-rooted volume's path space.
//! - [`path_prefix`]: component-aware absolute-path prefix tests (so `/a/bc` is
//!   never a child of `/a/b`), shared by rescan ancestor-collapse and
//!   removal-storm coalescing, plus the parent / ancestor-chain arithmetic the
//!   live-event and size-refresh paths run on.
//! - [`app_text`]: the crossings from a volume's app path (its root, a listed row) to the
//!   text the index stores and the native path its local pipeline opens, and back.
//!
//! Firmlink and `/private`-symlink normalization to the canonical form the index
//! stores lives one layer down, in `cmdr_fs::firmlinks`.

pub(crate) mod app_text;
pub(crate) mod path_prefix;
pub(crate) mod path_space;
pub(crate) mod routing;
