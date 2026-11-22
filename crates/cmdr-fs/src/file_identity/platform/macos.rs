//! `st_dev`, `st_ino`, and `st_nlink`, shared with the other Unix.

pub(crate) use super::unix::{device_at, device_of, file_id, file_id_at, file_id_of_open, link_count};
