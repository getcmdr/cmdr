//! Linux: the POSIX errnos and nothing more. Its `EOPNOTSUPP` is `ENOTSUP`, and a
//! missing xattr is `ENODATA`, which no caller tells apart.

use super::super::OsErrorClass;
use super::unix::POSIX;

pub(crate) const TABLES: &[&[(i32, OsErrorClass)]] = &[&POSIX];

/// No code here reads differently on a network share.
pub(crate) const ON_A_SHARE: &[(i32, OsErrorClass)] = &[];

/// Never asked: [`ON_A_SHARE`] is empty here.
pub(crate) fn is_on_a_share(_path: &str) -> bool {
    false
}

pub(crate) fn class_of(code: i32) -> OsErrorClass {
    super::class_in(TABLES, code)
}

pub(crate) fn code_of(class: OsErrorClass) -> Option<i32> {
    super::code_in(TABLES, class)
}
