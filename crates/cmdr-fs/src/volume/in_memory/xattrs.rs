//! `Volume`'s xattr methods on the store: the reference answer to what
//! `conformance::assert_xattrs_keep_their_promises` asks.

use super::InMemoryVolume;
use crate::app_path::{AppPath, AppPathBuf};
use crate::ignore_poison::RwLockIgnorePoison;
use crate::metadata::Xattr;
use crate::volume::{VolumeError, XattrAnswer, XattrReadMode};

impl InMemoryVolume {
    pub(super) fn read_xattr_many_impl(
        &self,
        paths: &[AppPathBuf],
        name: &str,
        mode: XattrReadMode,
    ) -> Vec<XattrAnswer> {
        let entries = self.entries.read_ignore_poison();
        paths
            .iter()
            .map(|path| match entries.get(&*self.normalize(path)) {
                Some(entry) => Ok(entry
                    .extra
                    .xattrs
                    .iter()
                    .find(|xattr| xattr.name == *name)
                    .map(|xattr| xattr.value.clone())),
                None if mode == XattrReadMode::AssumePathsExist => Ok(None),
                None => Err(VolumeError::NotFound(super::shown(path))),
            })
            .collect()
    }

    pub(super) fn write_xattr_impl(&self, path: &AppPath, name: &str, value: &[u8]) -> Result<(), VolumeError> {
        let mut entries = self.entries.write_ignore_poison();
        let entry = entries
            .get_mut(&*self.normalize(path))
            .ok_or_else(|| VolumeError::NotFound(super::shown(path)))?;
        entry.extra.xattrs.retain(|xattr| xattr.name != *name);
        entry.extra.xattrs.push(Xattr {
            name: name.into(),
            value: value.to_vec(),
        });
        Ok(())
    }

    pub(super) fn remove_xattr_impl(&self, path: &AppPath, name: &str) -> Result<(), VolumeError> {
        let mut entries = self.entries.write_ignore_poison();
        let entry = entries
            .get_mut(&*self.normalize(path))
            .ok_or_else(|| VolumeError::NotFound(super::shown(path)))?;
        entry.extra.xattrs.retain(|xattr| xattr.name != *name);
        Ok(())
    }
}
