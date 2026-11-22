//! Windows: a file's owner is a SID, never a numeric uid or gid, so no number names anyone here,
//! and the local reader leaves the owner empty there (`cmdr-local`'s `reading/platform/windows.rs`
//! says why).

pub(crate) fn user_name_of(_uid: u32) -> Option<String> {
    None
}

pub(crate) fn group_name_of(_gid: u32) -> Option<String> {
    None
}
