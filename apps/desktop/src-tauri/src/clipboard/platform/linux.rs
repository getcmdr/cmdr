//! Linux: the clipboard's plain text through `arboard` (`arboard_text.rs`); no file reader or
//! writer yet (`text/uri-list` is the one to add).

pub(crate) use super::arboard_text::read_text;
pub(crate) use super::unsupported::{
    after_cut_paste, app_text, drop_cut_mark, paste_as_file, read_file_urls, write_file_urls,
};
