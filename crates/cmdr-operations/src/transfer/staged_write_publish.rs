//! How a staged write of generated output replaces a file at its name whole:
//! one atomic rename-with-force ([`StagedWrite::commit_atomic_replace`]), or,
//! on a backend without one, the original set aside, the temp renamed onto the
//! free name, and the aside discarded
//! ([`StagedWrite::commit_with_displaced_original`]). A fresh compress over an
//! existing archive publishes this way. Split out of `staged_write.rs`, beside
//! the landing every other staged write takes (`staged_write_landing.rs`).
//!
//! ❗ **Classify before acting**, as the landing does: a publish rename that
//! fails on the transport may have been performed by the server, so the
//! publish is cut off right there ([`PublishError::CutOff`]), the temp left
//! where it is and the aside still held. Deleting the temp on that error
//! reported a failure for an archive that had been published, and removed
//! nothing. The caller waits for the destination and then settles
//! ([`CutOffPublish::resume`]) or gives up ([`CutOffPublish::give_up`]).

use std::sync::Arc;

use super::super::super::state::WriteOperationState;
use super::super::super::types::RecoveredOriginal;
use super::super::volume::{DisplacedDestination, displace_destination};
use super::landing::holds_our_size;
use super::{Spelled, StagedWrite, Temp, home_of};
use cmdr_fs::log_error::LogError;
use cmdr_fs::log_private::LogPrivate;
use cmdr_fs::volume::{Volume, VolumeError};

/// Why a publish didn't give the bytes their name.
#[derive(Debug)]
pub(crate) enum PublishError {
    /// It ended, settled: the original is at its name (or where `recovered`
    /// says), and the temp is gone, or stays listed for the sweeps when the
    /// destination wouldn't take its delete.
    Failed(PublishFailure),
    /// The publish rename met a transport fault (`VolumeError::transport_fault`),
    /// so the server may or may not have performed it. Nothing is settled yet.
    CutOff(Box<CutOffPublish>),
}

/// A publish that ended without the new bytes at the name.
#[derive(Debug)]
pub(crate) struct PublishFailure {
    /// What the destination said.
    pub(crate) error: VolumeError,
    /// The original set aside for the publish couldn't go back to its name,
    /// and is here instead: the path the user has to be told
    /// (`WriteOperationError::OriginalsKeptAside`).
    pub(crate) recovered: Option<RecoveredOriginal>,
}

/// How the publish takes the name from what's there.
enum Replacing {
    /// One rename-with-force (`Volume::supports_atomic_replace_rename`): the
    /// original is never removed in a separate step.
    Atomically,
    /// The original set aside (`None`: nothing was there), so the rename onto
    /// the free name can't clobber anything.
    SetAside(Option<Box<DisplacedDestination>>),
}

/// One publish in flight: the temp, where it's going, and how.
struct Publish {
    state: Arc<WriteOperationState>,
    temp: Temp,
    final_path: Spelled,
    /// The byte count the write confirmed: what a whole temp holds.
    size: u64,
    /// The staged write's ledger home.
    home_volume: Option<String>,
    replacing: Replacing,
}

impl StagedWrite {
    /// Atomically replaces the final name with a completed generated file.
    ///
    /// Only on a backend whose rename-with-force is one atomic replacement
    /// primitive (local POSIX, SMB's `ReplaceIfExists`), so the original is
    /// never removed in a separate operation. A refusal leaves the original
    /// intact and takes the staged bytes away. `size` is the byte count the
    /// write confirmed, which is how a cut-off publish's settle tells a whole
    /// temp from a short one.
    pub(crate) async fn commit_atomic_replace(
        mut self,
        dest_volume: &Arc<dyn Volume>,
        size: u64,
    ) -> Result<(), PublishError> {
        let Some(temp) = self.temp.take() else {
            return Err(PublishError::Failed(PublishFailure {
                error: VolumeError::NotSupported,
                recovered: None,
            }));
        };
        self.publish(temp, size, Replacing::Atomically)
            .rename(dest_volume)
            .await
    }

    /// Publishes on a backend without atomic replacement while keeping the old
    /// destination recoverable under a tracked aside. This is the MTP-safe
    /// fallback: the old file is restored if the final rename refuses, and is
    /// discarded only after the new file has its final name.
    pub(crate) async fn commit_with_displaced_original(
        mut self,
        dest_volume: &Arc<dyn Volume>,
        size: u64,
    ) -> Result<(), PublishError> {
        let Some(temp) = self.temp.take() else {
            return Err(PublishError::Failed(PublishFailure {
                error: VolumeError::NotSupported,
                recovered: None,
            }));
        };
        match displace_destination(&self.state, dest_volume, self.final_path.app(), false).await {
            Ok(displaced) => {
                self.publish(temp, size, Replacing::SetAside(displaced.map(Box::new)))
                    .rename(dest_volume)
                    .await
            }
            Err(error) => {
                // Nothing was set aside, so the original is where it was and
                // the temp is a copy nobody needs.
                let publish = self.publish(temp, size, Replacing::SetAside(None));
                Err(PublishError::Failed(publish.refused(dest_volume, error.error).await))
            }
        }
    }

    fn publish(&self, temp: Temp, size: u64, replacing: Replacing) -> Publish {
        Publish {
            state: Arc::clone(&self.state),
            temp,
            final_path: self.final_path.clone(),
            size,
            home_volume: self.home_volume.clone(),
            replacing,
        }
    }
}

impl Publish {
    /// The rename that publishes the bytes.
    async fn rename(self, dest_volume: &Arc<dyn Volume>) -> Result<(), PublishError> {
        let force = matches!(self.replacing, Replacing::Atomically);
        match dest_volume.rename(self.temp.app(), self.final_path.app(), force).await {
            Ok(()) => {
                self.published(dest_volume).await;
                Ok(())
            }
            Err(error) if error.transport_fault().is_some() => {
                log::info!(
                    target: "copy",
                    "publish: {} was cut off on the transport ({error}); settling it once the destination is back",
                    LogPrivate::file_path(self.final_path.path()), error = LogError(&error),
                );
                Err(PublishError::CutOff(Box::new(CutOffPublish { publish: self, error })))
            }
            Err(error) => Err(PublishError::Failed(self.refused(dest_volume, error).await)),
        }
    }

    /// The bytes have the name: the temp stops being anything of ours, and
    /// the original set aside for them goes.
    async fn published(self, dest_volume: &Arc<dyn Volume>) {
        super::super::super::in_flight_temps::retire_temp(
            &self.state,
            home_of(self.home_volume.as_deref()),
            self.temp.path(),
        );
        if let Replacing::SetAside(Some(displaced)) = self.replacing {
            displaced.discard(dest_volume).await;
        }
    }

    /// The publish is over without the bytes at the name: the original comes
    /// back from its aside, and the temp, a copy nobody needs (the output can
    /// be made again), goes. One the destination won't delete stays listed.
    async fn refused(self, dest_volume: &Arc<dyn Volume>, error: VolumeError) -> PublishFailure {
        let recovered = match self.replacing {
            Replacing::SetAside(Some(displaced)) => displaced
                .restore(dest_volume)
                .await
                .map(|kept_at| RecoveredOriginal::new(self.final_path.path(), &kept_at)),
            Replacing::SetAside(None) | Replacing::Atomically => None,
        };
        match dest_volume.delete(self.temp.app()).await {
            Ok(()) | Err(VolumeError::NotFound(_)) => super::super::super::in_flight_temps::retire_temp(
                &self.state,
                home_of(self.home_volume.as_deref()),
                self.temp.path(),
            ),
            Err(e) => log::debug!(
                target: "copy",
                "publish: couldn't remove the unpublished temp {}, leaving it listed: {e}",
                LogPrivate::file_path(self.temp.path()), e = LogError(&e),
            ),
        }
        PublishFailure { error, recovered }
    }
}

/// A publish a transport fault cut off ([`PublishError::CutOff`]): the server
/// may or may not have performed its rename.
pub(crate) struct CutOffPublish {
    publish: Publish,
    error: VolumeError,
}

impl std::fmt::Debug for CutOffPublish {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CutOffPublish")
            .field("temp", &self.publish.temp.path())
            .field("final_path", &self.publish.final_path.path())
            .field("error", &self.error)
            .finish()
    }
}

/// What settling a cut-off publish came to.
#[derive(Debug)]
pub(crate) enum Republished {
    /// The new bytes are at the name.
    Published,
    /// Nothing proves the publish happened and nothing of it is left to
    /// publish: the original is back at its name, so make the output again.
    RunAgain,
    /// It ended without publishing.
    Failed(PublishFailure),
    /// Cut off again: wait once more.
    CutOff(Box<CutOffPublish>),
}

impl From<Result<(), PublishError>> for Republished {
    fn from(published: Result<(), PublishError>) -> Self {
        match published {
            Ok(()) => Self::Published,
            Err(PublishError::Failed(failure)) => Self::Failed(failure),
            Err(PublishError::CutOff(cut_off)) => Self::CutOff(cut_off),
        }
    }
}

impl CutOffPublish {
    /// What cut it off.
    pub(crate) fn error(&self) -> &VolumeError {
        &self.error
    }

    /// The destination is back: settles what the cut-off rename did.
    ///
    /// - **The temp is there and whole**: the rename never happened, so it
    ///   runs again.
    /// - **The temp is gone**: nothing of ours removes it before this settles,
    ///   so the server performed the rename when the name holds a file of
    ///   exactly the size we wrote: published. Anything else at the name, and
    ///   nothing proves it.
    /// - **Nothing proves it** (that, or a temp that isn't whole): the
    ///   original goes back from its aside and the temp goes; run again,
    ///   unless the original couldn't go back (then it's reported where it is).
    pub(crate) async fn resume(self, dest_volume: &Arc<dyn Volume>) -> Republished {
        let Self { publish, error } = self;
        match dest_volume.get_metadata(publish.temp.app()).await {
            Ok(entry) if !entry.is_directory && !entry.is_symlink && entry.size == Some(publish.size) => {
                publish.rename(dest_volume).await.into()
            }
            Err(VolumeError::NotFound(_))
                if holds_our_size(dest_volume, publish.final_path.app(), publish.size).await =>
            {
                log::info!(
                    target: "copy",
                    "publish: {} was published before the drop swallowed the reply; counting it done",
                    LogPrivate::file_path(publish.final_path.path()),
                );
                publish.published(dest_volume).await;
                Republished::Published
            }
            Ok(_) | Err(VolumeError::NotFound(_)) => {
                let failure = publish.refused(dest_volume, error).await;
                match failure.recovered {
                    Some(_) => Republished::Failed(failure),
                    None => Republished::RunAgain,
                }
            }
            Err(stat) if stat.transport_fault().is_some() => {
                Republished::CutOff(Box::new(Self { publish, error: stat }))
            }
            Err(_) => Republished::Failed(publish.refused(dest_volume, error).await),
        }
    }

    /// The wait ended without the destination: it gave up, the person
    /// cancelled, or there was nothing to wait for. The rename's error meets
    /// the refusal rules: the original goes back (or is reported where it is),
    /// and the temp goes, or stays listed for the sweeps.
    pub(crate) async fn give_up(self, dest_volume: &Arc<dyn Volume>) -> PublishFailure {
        self.publish.refused(dest_volume, self.error).await
    }
}
