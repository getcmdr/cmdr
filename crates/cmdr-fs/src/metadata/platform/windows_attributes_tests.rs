use super::{HIDDEN, NORMAL, READONLY, attributes_adding, flags_of_attributes, settable_flags};
use crate::metadata::UserFlags;

const ARCHIVE: u32 = 0x20;
const DIRECTORY: u32 = 0x10;
const REPARSE_POINT: u32 = 0x400;

#[test]
fn a_read_only_file_is_locked_and_a_read_only_folder_is_not() {
    assert_eq!(flags_of_attributes(READONLY | ARCHIVE, true), UserFlags::LOCKED);
    assert_eq!(
        flags_of_attributes(READONLY | DIRECTORY, false),
        UserFlags::NONE,
        "a folder's read-only bit is Explorer's customization marker"
    );
    assert_eq!(flags_of_attributes(ARCHIVE, true), UserFlags::NONE);
}

#[test]
fn hidden_is_hidden_on_a_file_and_a_folder() {
    assert_eq!(flags_of_attributes(HIDDEN, true), UserFlags::HIDDEN);
    assert_eq!(flags_of_attributes(HIDDEN | DIRECTORY, false), UserFlags::HIDDEN);
    assert_eq!(
        flags_of_attributes(HIDDEN | READONLY, true),
        UserFlags::HIDDEN | UserFlags::LOCKED
    );
}

#[test]
fn only_hidden_and_a_files_locked_can_be_put_on() {
    let every = UserFlags::HIDDEN | UserFlags::LOCKED | UserFlags::APPEND_ONLY | UserFlags::NO_DUMP;
    assert_eq!(settable_flags(every, true), UserFlags::HIDDEN | UserFlags::LOCKED);
    assert_eq!(settable_flags(every, false), UserFlags::HIDDEN);
    assert_eq!(settable_flags(UserFlags::NO_DUMP, true), UserFlags::NONE);
}

#[test]
fn adding_keeps_what_the_item_wears_and_drops_what_the_call_cant_set() {
    assert_eq!(attributes_adding(ARCHIVE, UserFlags::LOCKED, true), ARCHIVE | READONLY);
    assert_eq!(
        attributes_adding(DIRECTORY | REPARSE_POINT, UserFlags::HIDDEN | UserFlags::LOCKED, false),
        HIDDEN,
        "a folder takes Hidden, never Locked, and the filesystem's own bits stay out of the write"
    );
    assert_eq!(attributes_adding(HIDDEN, UserFlags::HIDDEN, true), HIDDEN);
}

#[test]
fn an_empty_word_is_normal() {
    assert_eq!(attributes_adding(DIRECTORY, UserFlags::LOCKED, false), NORMAL);
    assert_eq!(attributes_adding(0, UserFlags::NONE, true), NORMAL);
}
