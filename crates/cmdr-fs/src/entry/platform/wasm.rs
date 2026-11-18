//! The browser build has no user database, so owners show as their numeric id.

pub(crate) fn user_name_of(_uid: u32) -> Option<String> {
    None
}

pub(crate) fn group_name_of(_gid: u32) -> Option<String> {
    None
}
