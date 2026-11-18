//! Windows: a file's owner is a SID, never a numeric uid or gid, so no number names anyone here.
//! The owner column's Windows answer (the SID's account name) is the windows-port plan's task 35.

pub(crate) fn user_name_of(_uid: u32) -> Option<String> {
    None
}

pub(crate) fn group_name_of(_gid: u32) -> Option<String> {
    None
}
