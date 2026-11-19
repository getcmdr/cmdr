//! Windows: the clipboard's plain text through `arboard` (`arboard_text.rs`); no file reader or
//! writer yet (`CF_HDROP`, with `Preferred DropEffect` for a cut, is the one to add: task 42).

pub(crate) use super::arboard_text::read_text;
pub(crate) use super::unsupported::{paste_as_file, read_file_urls, write_file_urls};
