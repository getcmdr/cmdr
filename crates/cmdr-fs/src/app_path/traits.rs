//! The app path types' standard traits: borrowing and ownership (`Deref`, `Borrow`,
//! `ToOwned`), comparison (component-wise, `shape::components`), and text (`Display` is the
//! wire spelling, `Debug` names the type). ❌ Nothing here converts to or from `std::path`:
//! `tests.rs` § "No conversion to `std::path`" holds that at compile time.

use std::borrow::Borrow;
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;

use super::{AppName, AppNameBuf, AppPath, AppPathBuf, RelativeAppPath, RelativeAppPathBuf, shape};

// ---- Borrowing and ownership ----

impl Deref for AppPathBuf {
    type Target = AppPath;
    fn deref(&self) -> &AppPath {
        self.as_app_path()
    }
}

impl Borrow<AppPath> for AppPathBuf {
    fn borrow(&self) -> &AppPath {
        self.as_app_path()
    }
}

impl ToOwned for AppPath {
    type Owned = AppPathBuf;
    fn to_owned(&self) -> AppPathBuf {
        AppPathBuf(self.0.to_owned())
    }
}

impl Deref for RelativeAppPathBuf {
    type Target = RelativeAppPath;
    fn deref(&self) -> &RelativeAppPath {
        RelativeAppPath::new_unchecked(&self.0)
    }
}

impl Borrow<RelativeAppPath> for RelativeAppPathBuf {
    fn borrow(&self) -> &RelativeAppPath {
        self
    }
}

impl ToOwned for RelativeAppPath {
    type Owned = RelativeAppPathBuf;
    fn to_owned(&self) -> RelativeAppPathBuf {
        RelativeAppPathBuf(self.0.to_owned())
    }
}

impl fmt::Debug for RelativeAppPathBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl Deref for AppNameBuf {
    type Target = AppName;
    fn deref(&self) -> &AppName {
        self.as_app_name()
    }
}

impl Borrow<AppName> for AppNameBuf {
    fn borrow(&self) -> &AppName {
        self.as_app_name()
    }
}

impl ToOwned for AppName {
    type Owned = AppNameBuf;
    fn to_owned(&self) -> AppNameBuf {
        AppNameBuf(self.0.to_owned())
    }
}

// ---- Comparison: component-wise, as `PathBuf` on POSIX (`shape::components`) ----

impl PartialEq for AppPath {
    fn eq(&self, other: &AppPath) -> bool {
        self.0 == other.0 || shape::components(&self.0).eq(shape::components(&other.0))
    }
}

impl Eq for AppPath {}

impl Hash for AppPath {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for component in shape::components(&self.0) {
            component.hash(state);
        }
    }
}

impl PartialOrd for AppPath {
    fn partial_cmp(&self, other: &AppPath) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AppPath {
    fn cmp(&self, other: &AppPath) -> Ordering {
        if self.0 == other.0 {
            return Ordering::Equal;
        }
        shape::components(&self.0).cmp(shape::components(&other.0))
    }
}

impl PartialEq for AppPathBuf {
    fn eq(&self, other: &AppPathBuf) -> bool {
        self.as_app_path() == other.as_app_path()
    }
}

impl Eq for AppPathBuf {}

impl Hash for AppPathBuf {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_app_path().hash(state);
    }
}

impl PartialOrd for AppPathBuf {
    fn partial_cmp(&self, other: &AppPathBuf) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AppPathBuf {
    fn cmp(&self, other: &AppPathBuf) -> Ordering {
        self.as_app_path().cmp(other.as_app_path())
    }
}

impl PartialEq<AppPath> for AppPathBuf {
    fn eq(&self, other: &AppPath) -> bool {
        self.as_app_path() == other
    }
}

impl PartialEq<AppPathBuf> for AppPath {
    fn eq(&self, other: &AppPathBuf) -> bool {
        self == other.as_app_path()
    }
}

impl PartialEq for RelativeAppPath {
    fn eq(&self, other: &RelativeAppPath) -> bool {
        self.0 == other.0
    }
}

impl Eq for RelativeAppPath {}

impl Hash for RelativeAppPath {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl PartialEq for AppName {
    fn eq(&self, other: &AppName) -> bool {
        self.0 == other.0
    }
}

impl Eq for AppName {}

// By the text, the same as `AppNameBuf`'s derived impls, so a `HashMap<AppNameBuf, _>` or a
// `BTreeMap` is looked up by `&AppName` through `Borrow`.
impl Hash for AppName {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl PartialOrd for AppName {
    fn partial_cmp(&self, other: &AppName) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AppName {
    fn cmp(&self, other: &AppName) -> Ordering {
        self.0.cmp(&other.0)
    }
}

// ---- Text ----

/// The wire spelling, for logs (inside `LogPrivate`) and the wire. ❌ Never what a person
/// reads: that's [`AppPath::shown`].
impl fmt::Display for AppPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for AppPathBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for AppPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AppPath({:?})", &self.0)
    }
}

impl fmt::Debug for AppPathBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_app_path(), f)
    }
}

impl fmt::Display for RelativeAppPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for RelativeAppPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RelativeAppPath({:?})", &self.0)
    }
}

impl fmt::Display for AppName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for AppName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AppName({:?})", &self.0)
    }
}

impl fmt::Debug for AppNameBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_app_name(), f)
    }
}
