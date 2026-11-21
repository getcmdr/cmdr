//! "Go to path" backend, and the app's typed-path resolver.
//!
//! The dialog lets the user type a path (absolute, `~`-rooted, or relative to the
//! focused pane) and jump to it. All the path reasoning lives here so the frontend
//! stays a thin presenter (AGENTS.md principle 3, "smart backend, thin frontend").
//! The IPC wrappers in `commands/go_to_path.rs` are pass-throughs.
//!
//! [`AppHost::resolve_typed`] is the ONE resolver for text a person or an agent typed: Go to
//! path, the `cmdr` CLI's paths, deep links, and the MCP tools' path inputs
//! (`tools/agent_path.rs`). It
//! runs `cmdr_fs::typed_path` over [`AppHost`]: the volume registry picks the volume
//! holding a path and answers each `..` through `Volume::parent_for_dot_dot`, under a
//! timeout, so a hung mount refuses instead of blocking. Decisions: `DETAILS.md`.

pub mod history;

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use cmdr_fs::app_path::{AppPath, AppPathBuf, PathFlavor, PathKind};
use cmdr_fs::deadline::{Deadline, timeout_detached_within};
use cmdr_fs::go_to_path::{FoundKind, GoToHost};
use cmdr_fs::typed_path::{TypedBy, TypedPath, TypedPathHost, TypedPathRefusal, resolve_typed_with};
use cmdr_fs::volume::{DEFAULT_VOLUME_ID, Volume, VolumeError};

use crate::file_system::volume::manager::get_volume_manager;

/// The contract the dialog branches on, shared with the browser demo's engine:
/// `cmdr_fs::go_to_path`, where the classification lives.
pub use cmdr_fs::go_to_path::GoToPathResolution;

/// One resolve's whole budget, every `..` and stat inside it sharing it (the 2 s read tier,
/// `commands/CLAUDE.md`): the inner layer of the two-layer timeout, under the frontend's
/// race, so the refusal is ours and typed.
const RESOLVE_DEADLINE: Duration = Duration::from_secs(2);

/// Which volume holds a path: the registry in the app, a fixed volume in a test.
type VolumeFor = dyn Fn(&AppPath) -> Option<Arc<dyn Volume>> + Send + Sync;

/// The app as `cmdr_fs::typed_path` and `cmdr_fs::go_to_path` ask it, for ONE resolve.
pub(crate) struct AppHost {
    volume_for: Box<VolumeFor>,
    deadline: Deadline,
}

impl AppHost {
    /// Over the volume registry, with a fresh [`RESOLVE_DEADLINE`].
    pub(crate) fn registry() -> Self {
        Self::over(registry_volume_for, RESOLVE_DEADLINE)
    }

    /// Over ONE volume, every `..` its answer, with a fresh [`RESOLVE_DEADLINE`]: for text
    /// whose volume the caller already picked, like the transfer dialog's destination box,
    /// where the volume that will do the write is the one that says what its `..` means.
    pub(crate) fn on_volume(volume: Arc<dyn Volume>) -> Self {
        Self::over(move |_| Some(Arc::clone(&volume)), RESOLVE_DEADLINE)
    }

    /// Over `volume_for`, every answer inside `budget` from now.
    pub(crate) fn over(
        volume_for: impl Fn(&AppPath) -> Option<Arc<dyn Volume>> + Send + Sync + 'static,
        budget: Duration,
    ) -> Self {
        Self {
            volume_for: Box::new(volume_for),
            deadline: Deadline::new(budget),
        }
    }

    /// The volume holding `path`, or `NotConnected` when none is registered (a server
    /// nobody connected): no volume to ask means no answer, ❌ never a lexical guess.
    fn volume(&self, path: &AppPath) -> Result<Arc<dyn Volume>, VolumeError> {
        (self.volume_for)(path).ok_or_else(|| VolumeError::NotConnected(path.to_string()))
    }

    /// The root of the volume holding `path`, `None` when none does: what the MCP tools' path
    /// door (`tools/agent_path.rs`) compares to tell a link that leads onto another volume.
    pub(crate) fn root_holding(&self, path: &AppPath) -> Option<AppPathBuf> {
        (self.volume_for)(path).map(|volume| volume.root().to_owned())
    }

    /// `work` within what's left of the deadline. Detached on expiry (the MTP rule,
    /// `cmdr_fs::deadline`): the caller gets `ConnectionTimeout` at once, and the work
    /// runs on to its own end rather than being dropped mid-transaction.
    pub(crate) async fn within<T: Send + 'static>(
        &self,
        work: impl Future<Output = Result<T, VolumeError>> + Send + 'static,
    ) -> Result<T, VolumeError> {
        timeout_detached_within(
            &self.deadline,
            || VolumeError::ConnectionTimeout("the volume didn't answer in time".into()),
            |detail| VolumeError::IoError {
                message: detail.into(),
                raw_os_error: None,
            },
            work,
        )
        .await
    }
}

/// The registered volume whose root holds `path`, the longest root winning (a mount
/// inside another), routed volumes (archives, `.git` portals) skipped: they route over
/// the disk that holds them. In memory, no syscall and no dial.
fn registry_volume_for(path: &AppPath) -> Option<Arc<dyn Volume>> {
    let manager = get_volume_manager();
    let held = manager
        .list_volumes_with_handles()
        .into_iter()
        .filter(|(_, volume)| !volume.routes_over_a_parent() && path.is_on_volume(volume.root()))
        .max_by_key(|(_, volume)| volume.root().as_str().len())
        .map(|(_, volume)| volume);
    // A local path always has a disk: the boot volume, when no registered root is closer.
    held.or_else(|| {
        (path.kind() != PathKind::Scheme)
            .then(|| manager.get(DEFAULT_VOLUME_ID))
            .flatten()
    })
}

impl TypedPathHost for AppHost {
    fn flavor(&self) -> PathFlavor {
        PathFlavor::HOST
    }

    fn expand_tilde(&self, text: &str) -> String {
        cmdr_fs::tilde::expand_tilde(text)
    }

    async fn parent_for_dot_dot(&self, path: &AppPath) -> Result<AppPathBuf, VolumeError> {
        let volume = self.volume(path)?;
        let path = path.to_owned();
        self.within(async move { volume.parent_for_dot_dot(&path).await }).await
    }
}

impl GoToHost for AppHost {
    /// A local path through `metadata` (follows links, so a link to a folder is a
    /// folder), off the async thread; a remote one through its volume. Either within
    /// the deadline, since a hung mount blocks `metadata` for minutes. A missed deadline,
    /// or no volume to ask, is `Err` ("couldn't check"); a stat that answers with an
    /// error (missing, refused) is nothing there, as it always was.
    async fn found(&self, path: &AppPath) -> Result<Option<FoundKind>, VolumeError> {
        let is_directory = match path.to_native() {
            Ok(native) => {
                self.within(async move {
                    let stat = tokio::task::spawn_blocking(move || std::fs::metadata(native).map(|meta| meta.is_dir()));
                    Ok(stat.await.ok().and_then(Result::ok))
                })
                .await?
            }
            Err(_) => {
                let volume = self.volume(path)?;
                let path = path.to_owned();
                match self.within(async move { volume.is_directory(&path).await }).await {
                    Ok(is_directory) => Some(is_directory),
                    Err(
                        err @ (VolumeError::ConnectionTimeout(_)
                        | VolumeError::NotConnected(_)
                        | VolumeError::DeviceDisconnected(_)
                        | VolumeError::DeviceSessionReset(_)),
                    ) => return Err(err),
                    Err(_) => None,
                }
            }
        };
        Ok(is_directory.map(|is_directory| {
            if is_directory {
                FoundKind::Directory
            } else {
                FoundKind::File
            }
        }))
    }

    /// A path that crosses into a supported archive is browsed as a folder, even though the
    /// inner path (or the `.zip` file itself) isn't a real directory on disk, so typing it
    /// in the path bar enters the archive. The boundary check confirms a real archive FILE +
    /// zip magic, so a directory named `foo.zip` or a mislabeled file still classifies
    /// normally. Local disks only.
    async fn enters_archive(&self, path: &AppPath) -> bool {
        let Ok(native) = path.to_native() else {
            return false;
        };
        let check = self.within(async move {
            let crosses = tokio::task::spawn_blocking(move || cmdr_archive::path_crosses_archive_boundary(&native));
            Ok(crosses.await.unwrap_or(false))
        });
        matches!(check.await, Ok(true))
    }
}

impl AppHost {
    /// Resolves text a person or an agent typed (`by`) against `base` (the pane's folder,
    /// for a relative input), every `..` answered by the volume holding the path: the one
    /// door for Go to path, the CLI's paths, deep links, and the MCP tools' path inputs
    /// (`AppHost::registry().resolve_typed(…)`). An agent's `..` above a root is
    /// [`TypedPathRefusal::OffRoot`].
    ///
    /// # Errors
    ///
    /// [`TypedPathRefusal`], typed.
    pub(crate) async fn resolve_typed(
        &self,
        text: &str,
        base: Option<&AppPath>,
        by: TypedBy,
    ) -> Result<TypedPath, TypedPathRefusal> {
        resolve_typed_with(text, base, by, self).await
    }
}

/// Resolves the dialog's `input` against `base_dir` (the focused pane's path, as the
/// frontend holds it) into a [`GoToPathResolution`]. A pane path that's no app path
/// reads a relative input as nothing to go to.
pub async fn resolve(input: &str, base_dir: &str) -> GoToPathResolution {
    resolve_over(input, base_dir, &AppHost::registry()).await
}

async fn resolve_over(input: &str, base_dir: &str, host: &AppHost) -> GoToPathResolution {
    let base = AppPathBuf::from_wire(base_dir).ok();
    let typed = host.resolve_typed(input.trim(), base.as_deref(), TypedBy::Person).await;
    cmdr_fs::go_to_path::answer(typed, host).await
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod wire_shape_tests;
