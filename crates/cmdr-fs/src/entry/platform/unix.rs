//! The passwd and group databases, through `uzers`, shared by both Unixes.

pub(crate) fn user_name_of(uid: u32) -> Option<String> {
    uzers::get_user_by_uid(uid).map(|u| u.name().to_string_lossy().into_owned())
}

pub(crate) fn group_name_of(gid: u32) -> Option<String> {
    uzers::get_group_by_gid(gid).map(|g| g.name().to_string_lossy().into_owned())
}
