//! Cmdr's directory listing engine in a browser: `cmdr-listing` over an
//! `InMemoryVolume`, answering the listing commands the way the desktop's Tauri
//! commands do. The website's `/demo` routes those commands here (#302).
//!
//! The engine's second host, beside `apps/desktop/src-tauri/src/listing_host/`:
//! [`tree`] builds the volume, [`events`] turns engine facts into Tauri-shaped
//! events, [`commands`] answers the commands (handing the write, queue, and
//! undo ones to [`operations_host`], the write engine's host), [`menu`] hosts
//! the menu bar (`cmdr-menu`), [`search`] answers ⌘F (`cmdr-search`), [`multi_rename`] answers ⌃M (`cmdr-multi-rename`), [`about`] answers "Tell me about this" (`cmdr-about`), and `bindings` (browser only) is the
//! `wasm-bindgen` surface. Everything but `bindings` builds natively too, so
//! the tests run on the host. `CLAUDE.md` has the must-knows, `DETAILS.md` the JS
//! API and the build.
#![warn(unused_crate_dependencies)]
#![deny(missing_docs)]

// Only here for its `js` feature: Multi-Rename's dates read the page's zone.
#[cfg(target_family = "wasm")]
use jiff as _;

pub mod about;
pub mod backend_listings;
#[cfg(target_family = "wasm")]
mod bindings;
pub mod commands;
pub mod context_menu;
pub mod events;
pub mod folder_count;
pub mod folder_sizes;
pub mod go_to;
pub mod imports;
mod invoke;
pub mod menu;
pub mod multi_rename;
pub mod operations_host;
#[cfg(target_family = "wasm")]
mod rt_self_test;
pub mod search;
pub mod selection_recents;
pub mod tree;
pub mod viewer;

#[cfg(all(test, not(target_family = "wasm")))]
mod about_test;
#[cfg(all(test, not(target_family = "wasm")))]
mod commands_test;
#[cfg(all(test, not(target_family = "wasm")))]
mod context_menu_test;
#[cfg(all(test, not(target_family = "wasm")))]
mod engine_command_names_test;
#[cfg(all(test, not(target_family = "wasm")))]
mod events_test;
#[cfg(all(test, not(target_family = "wasm")))]
mod folder_count_test;
#[cfg(all(test, not(target_family = "wasm")))]
mod folder_sizes_test;
#[cfg(all(test, not(target_family = "wasm")))]
mod go_to_test;
#[cfg(all(test, not(target_family = "wasm")))]
mod imports_test;
#[cfg(all(test, not(target_family = "wasm")))]
mod menu_test;
#[cfg(all(test, not(target_family = "wasm")))]
mod multi_rename_test;
#[cfg(all(test, not(target_family = "wasm")))]
mod search_test;
#[cfg(all(test, not(target_family = "wasm")))]
mod selection_recents_test;
#[cfg(all(test, not(target_family = "wasm")))]
mod tree_test;
#[cfg(all(test, not(target_family = "wasm")))]
mod viewer_test;
