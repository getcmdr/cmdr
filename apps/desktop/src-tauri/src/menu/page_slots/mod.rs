//! The OS-filled slots of a right-click menu a page draws (`page_context.rs`): what each OS
//! reads for them before the menu goes up, how it draws them, and what a pick needs kept.
//!
//! Only Windows draws its menus in the page, and of the slots it fills only "Open with", from the
//! Shell's handlers (`file_system/DETAILS.md` § "Open with on Windows"). macOS and Linux keep
//! native popups, so their arms fill nothing. `menu/DETAILS.md` § "Right-click menus a page
//! draws".

mod platform;

pub(crate) use platform::read;
