//! The lifecycle a connect and an edit owe the rest of the app: a volume in the
//! registry under the RESOLVED id, a place in the saved list with the resolved
//! split, a uid change that redials, a name change that doesn't, and nothing left
//! behind by a refusal or a cancel.
//!
//! Over a fake backend ([`FakeNfs`]) standing in for `cmdr_nfs`: an
//! `InMemoryVolume` per dial, keyed by its app root. The dial itself is the
//! crate's own cells (`crates/cmdr-nfs/DETAILS.md`). Each cell uses a host of its
//! own, since the store and the registry are process-global.

use cmdr_fs::app_path::AppPath;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cmdr_fs::volume::{BackendKind, Volume};
use cmdr_nfs::{NfsConnectError, NfsConnectionParams, NfsIdentity, NfsServer};
use tokio_util::sync::CancellationToken;

use super::{ATTEMPTS, Dialed, NfsBackend, NfsConnection, failed};
use crate::file_system::volume::InMemoryVolume;
use crate::file_system::volume::manager::get_volume_manager;
use crate::network::nfs_known_places::{self, KnownNfsPlace};
use crate::network::saved_server_fields::SavedServerOutcome;

/// What one fake dial answers.
enum Answer {
    /// The dial comes up, having resolved the typed export into this export and
    /// this start folder.
    Resolves {
        export: &'static str,
        folder: Option<&'static str>,
    },
    /// The dial refuses.
    Refuses(NfsConnectError),
    /// The dial waits for its cancel, then answers `Cancelled`, as the crate's does.
    Hangs,
}

/// A backend whose dials answer from a script, and whose volumes are in-memory.
#[derive(Default)]
struct FakeNfs {
    answers: Mutex<VecDeque<Answer>>,
    /// Every dial's params, in order.
    dialed: Mutex<Vec<NfsConnectionParams>>,
    /// The params each live fake volume dials with, by app root.
    live: Mutex<HashMap<String, NfsConnectionParams>>,
    /// Every "reconnect automatically" a live volume was told.
    switched: Mutex<Vec<bool>>,
}

impl FakeNfs {
    fn answering(answers: impl IntoIterator<Item = Answer>) -> Self {
        Self {
            answers: Mutex::new(answers.into_iter().collect()),
            ..Self::default()
        }
    }

    fn dial_count(&self) -> usize {
        self.dialed.lock().expect("test lock").len()
    }

    fn volume_for(&self, name: &str, params: NfsConnectionParams) -> Arc<dyn Volume> {
        let root = params.app_root();
        self.live.lock().expect("test lock").insert(root.clone(), params);
        Arc::new(
            InMemoryVolume::new(name)
                .with_backend_kind(BackendKind::Nfs)
                .with_root(root),
        )
    }
}

impl NfsBackend for FakeNfs {
    fn dial<'a>(
        &'a self,
        name: &'a str,
        params: NfsConnectionParams,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<Dialed, NfsConnectError>> + Send + 'a>> {
        Box::pin(async move {
            self.dialed.lock().expect("test lock").push(params.clone());
            let answer = self.answers.lock().expect("test lock").pop_front();
            match answer.expect("a scripted answer per dial") {
                Answer::Resolves { export, folder } => {
                    let mut resolved = NfsConnectionParams::new(params.server.clone(), export, params.identity);
                    resolved.start_folder = params.start_folder.clone().or(folder.map(str::to_string));
                    resolved.auto_reconnect = params.auto_reconnect;
                    Ok(Dialed {
                        volume: self.volume_for(name, resolved.clone()),
                        params: resolved,
                    })
                }
                Answer::Refuses(error) => Err(error),
                Answer::Hangs => {
                    cancel.cancelled().await;
                    Err(NfsConnectError::Cancelled)
                }
            }
        })
    }

    fn params_of(&self, volume: &dyn Volume) -> Option<NfsConnectionParams> {
        let root = volume.root().as_std_legacy().to_string_lossy().into_owned();
        self.live.lock().expect("test lock").get(&root).cloned()
    }

    fn sharing_connection(&self, volume: &dyn Volume, name: &str) -> Option<Arc<dyn Volume>> {
        let params = self.params_of(volume)?;
        Some(Arc::new(
            InMemoryVolume::new(name)
                .with_backend_kind(BackendKind::Nfs)
                .with_root(params.app_root()),
        ))
    }

    fn set_auto_reconnect(&self, _volume: &dyn Volume, on: bool) {
        self.switched.lock().expect("test lock").push(on);
    }

    fn set_start_folder(&self, volume: &dyn Volume, folder: Option<String>) {
        let root = volume.root().as_std_legacy().to_string_lossy().into_owned();
        if let Some(params) = self.live.lock().expect("test lock").get_mut(&root) {
            params.start_folder = folder;
        }
    }
}

fn place(host: &str, export: &str) -> KnownNfsPlace {
    KnownNfsPlace {
        host: host.to_string(),
        port: None,
        export: export.to_string(),
        uid: 1000,
        gid: 100,
        other_gids: Vec::new(),
        start_folder: None,
        auto_reconnect: true,
        pinned: false,
        last_connected_at: String::new(),
    }
}

fn id_of(host: &str, export: &str) -> String {
    place(host, export).volume_id()
}

async fn connect(backend: &FakeNfs, name: &str, place: KnownNfsPlace, attempt: &str) -> NfsConnection {
    super::connect_and_register_with(
        backend,
        crate::network::connect_wiring::DialTicket::now(),
        name,
        place,
        attempt,
    )
    .await
}

#[test]
fn every_connect_refusal_keeps_its_own_outcome() {
    // ❗ A missing export, a refused one, a missing reserved port, and a server
    // with no v3 each word differently, so none may collapse into another.
    assert_eq!(failed(NfsConnectError::ExportNotFound), NfsConnection::ExportNotFound);
    assert_eq!(failed(NfsConnectError::ExportRefused), NfsConnection::ExportRefused);
    assert_eq!(
        failed(NfsConnectError::ReservedPortRequired),
        NfsConnection::ReservedPortRequired
    );
    assert_eq!(failed(NfsConnectError::NoNfsV3), NfsConnection::NoNfsV3);
    assert_eq!(failed(NfsConnectError::TimedOut), NfsConnection::TimedOut);
    assert_eq!(failed(NfsConnectError::Cancelled), NfsConnection::Cancelled);
    assert_eq!(
        failed(NfsConnectError::Unreachable("x".to_string())),
        NfsConnection::Unreachable
    );
    assert_eq!(
        failed(NfsConnectError::Transport("x".to_string())),
        NfsConnection::Unreachable
    );
}

#[tokio::test]
async fn a_connect_registers_and_saves_under_the_resolved_export() {
    let backend = FakeNfs::answering([Answer::Resolves {
        export: "/share/Public",
        folder: Some("/photos"),
    }]);
    let typed = place("resolve.test", "/share/Public/photos");
    let outcome = connect(&backend, "Home NAS", typed, "nfs-resolve").await;

    let resolved_id = id_of("resolve.test", "/share/Public");
    assert_eq!(
        outcome,
        NfsConnection::Connected {
            volume_id: resolved_id.clone()
        }
    );
    let manager = get_volume_manager();
    assert!(manager.get(&resolved_id).is_some(), "registered under the export's id");
    let typed_id = id_of("resolve.test", "/share/Public/photos");
    assert!(manager.get(&typed_id).is_none(), "❌ never under the typed path's id");
    assert!(nfs_known_places::find(&typed_id).is_none(), "❌ nor saved under it");

    let saved = nfs_known_places::find(&resolved_id).expect("a connect remembers the place");
    assert_eq!(saved.export, "/share/Public");
    assert_eq!(
        saved.start_folder.as_deref(),
        Some("/photos"),
        "the rest is the start folder"
    );
    assert!(saved.pinned, "a first connect pins the new place");
    assert!(!saved.last_connected_at.is_empty());
    assert_eq!(
        nfs_known_places::server_name(&saved),
        "Home NAS",
        "the typed name names the SERVER"
    );
    assert_eq!(
        manager.get(&resolved_id).expect("registered").name(),
        "share/Public",
        "the volume reads as its resolved export"
    );

    assert!(super::disconnect_with(&backend, &resolved_id).await);
    assert!(manager.get(&resolved_id).is_none(), "a disconnect unregisters");
    assert!(
        nfs_known_places::find(&resolved_id).is_some(),
        "and keeps the place saved"
    );
    nfs_known_places::forget(&resolved_id);
}

#[tokio::test]
async fn a_refused_connect_registers_and_saves_nothing() {
    let backend = FakeNfs::answering([Answer::Refuses(NfsConnectError::ReservedPortRequired)]);
    let outcome = connect(&backend, "Refused", place("refused.test", "/secure"), "nfs-refused").await;
    assert_eq!(outcome, NfsConnection::ReservedPortRequired);
    let id = id_of("refused.test", "/secure");
    assert!(get_volume_manager().get(&id).is_none(), "nothing registered");
    assert!(nfs_known_places::find(&id).is_none(), "nothing saved");
    assert_eq!(
        nfs_known_places::server_name(&place("refused.test", "/")),
        "",
        "no name adopted"
    );
}

#[tokio::test]
async fn a_cancelled_connect_registers_and_saves_nothing() {
    let backend = Arc::new(FakeNfs::answering([Answer::Hangs]));
    let dialing = {
        let backend = Arc::clone(&backend);
        tokio::spawn(async move { connect(&backend, "Cancelled", place("cancel.test", "/Public"), "nfs-cancel").await })
    };
    crate::test_support::wait_until_async(Duration::from_secs(5), "the connect is cancelable", || {
        ATTEMPTS.is_filed("nfs-cancel")
    })
    .await;
    assert!(super::cancel_connect("nfs-cancel"));
    assert_eq!(dialing.await.expect("the connect ends"), NfsConnection::Cancelled);
    let id = id_of("cancel.test", "/Public");
    assert!(get_volume_manager().get(&id).is_none(), "nothing registered");
    assert!(nfs_known_places::find(&id).is_none(), "nothing saved");
    assert!(!ATTEMPTS.is_filed("nfs-cancel"), "the attempt is gone");
}

#[tokio::test]
async fn a_reconnect_keeps_the_pin_the_user_moved() {
    let backend = FakeNfs::answering([
        Answer::Resolves {
            export: "/Public",
            folder: None,
        },
        Answer::Resolves {
            export: "/Public",
            folder: None,
        },
    ]);
    let id = id_of("repin.test", "/Public");
    let _ = connect(&backend, "", place("repin.test", "/Public"), "nfs-repin-1").await;
    assert!(nfs_known_places::set_pinned(&id, false));
    assert!(super::disconnect_with(&backend, &id).await);
    let saved = nfs_known_places::find(&id).expect("saved");
    let outcome = super::connect_and_register_with(
        &backend,
        crate::network::connect_wiring::DialTicket::now(),
        "",
        saved,
        "nfs-repin-2",
    )
    .await;
    assert_eq!(outcome, NfsConnection::Connected { volume_id: id.clone() });
    assert!(
        !nfs_known_places::find(&id).expect("saved").pinned,
        "❌ a connect never re-pins"
    );
    assert!(super::disconnect_with(&backend, &id).await);
    nfs_known_places::forget(&id);
}

#[tokio::test]
async fn an_edit_to_a_place_nobody_has_open_saves_without_dialing() {
    let backend = FakeNfs::default();
    let id = id_of("offline-edit.test", "/Public");
    nfs_known_places::remember(KnownNfsPlace {
        pinned: true,
        ..place("offline-edit.test", "/Public")
    });
    let edit = KnownNfsPlace {
        uid: 1026,
        gid: 1026,
        other_gids: Vec::new(),
        start_folder: Some("photos/".to_string()),
        auto_reconnect: false,
        pinned: false,
        ..place("offline-edit.test", "/Public")
    };
    assert_eq!(
        super::save_without_connecting_with(&backend, edit, "Attic").await,
        SavedServerOutcome::Saved
    );
    assert_eq!(backend.dial_count(), 0, "nothing open, nothing dialed");
    let saved = nfs_known_places::find(&id).expect("saved");
    assert_eq!((saved.uid, saved.gid), (1026, 1026));
    assert_eq!(saved.start_folder.as_deref(), Some("/photos"), "normalized");
    assert!(!saved.auto_reconnect);
    assert!(saved.pinned, "❌ an edit never moves the pin");
    assert_eq!(nfs_known_places::server_name(&saved), "Attic");
    nfs_known_places::forget(&id);
}

#[tokio::test]
async fn a_changed_uid_on_an_open_place_redials_over_a_new_connection() {
    let backend = FakeNfs::answering([
        Answer::Resolves {
            export: "/Public",
            folder: None,
        },
        Answer::Resolves {
            export: "/Public",
            folder: None,
        },
    ]);
    let id = id_of("redial.test", "/Public");
    let _ = connect(&backend, "", place("redial.test", "/Public"), "nfs-redial").await;
    let before = get_volume_manager().get(&id).expect("connected");

    let edit = KnownNfsPlace {
        uid: 1026,
        gid: 1026,
        other_gids: vec![2001],
        ..place("redial.test", "/Public")
    };
    assert_eq!(
        super::save_without_connecting_with(&backend, edit, "").await,
        SavedServerOutcome::Saved
    );
    assert_eq!(backend.dial_count(), 2, "the new identity dialed its own connection");
    let redial = backend.dialed.lock().expect("test lock")[1].clone();
    assert_eq!(
        redial.identity,
        NfsIdentity {
            uid: 1026,
            gid: 1026,
            other_gids: vec![2001],
        }
    );
    let after = get_volume_manager().get(&id).expect("still connected, same id");
    assert!(!Arc::ptr_eq(&before, &after), "the new connection serves the place");
    let saved = nfs_known_places::find(&id).expect("saved");
    assert_eq!((saved.uid, saved.gid), (1026, 1026));

    assert!(super::disconnect_with(&backend, &id).await);
    nfs_known_places::forget(&id);
}

#[tokio::test]
async fn a_uid_the_server_refuses_saves_nothing_and_keeps_the_live_place() {
    let backend = FakeNfs::answering([
        Answer::Resolves {
            export: "/Public",
            folder: None,
        },
        Answer::Refuses(NfsConnectError::ExportRefused),
    ]);
    let id = id_of("refused-uid.test", "/Public");
    let _ = connect(&backend, "", place("refused-uid.test", "/Public"), "nfs-refused-uid").await;
    let before = get_volume_manager().get(&id).expect("connected");

    let edit = KnownNfsPlace {
        uid: 0,
        ..place("refused-uid.test", "/Public")
    };
    assert_eq!(
        super::save_without_connecting_with(&backend, edit, "").await,
        SavedServerOutcome::Unreachable,
        "an edit nobody could confirm"
    );
    let after = get_volume_manager().get(&id).expect("still connected");
    assert!(Arc::ptr_eq(&before, &after), "the live connection is untouched");
    assert_eq!(
        nfs_known_places::find(&id).expect("saved").uid,
        1000,
        "❗ nothing saved"
    );

    assert!(super::disconnect_with(&backend, &id).await);
    nfs_known_places::forget(&id);
}

#[tokio::test]
async fn a_changed_name_on_an_open_place_shares_its_connection() {
    let backend = FakeNfs::answering([Answer::Resolves {
        export: "/",
        folder: None,
    }]);
    let id = id_of("rename.test", "/");
    let _ = connect(&backend, "", place("rename.test", "/"), "nfs-rename").await;
    assert_eq!(get_volume_manager().get(&id).expect("connected").name(), "rename.test");

    let edit = KnownNfsPlace {
        auto_reconnect: false,
        ..place("rename.test", "/")
    };
    assert_eq!(
        super::save_without_connecting_with(&backend, edit, "Basement").await,
        SavedServerOutcome::Saved
    );
    assert_eq!(backend.dial_count(), 1, "❌ a name change never redials");
    assert_eq!(
        get_volume_manager().get(&id).expect("connected").name(),
        "Basement",
        "the root export reads as its newly named server"
    );
    assert_eq!(*backend.switched.lock().expect("test lock"), vec![false]);
    assert!(!nfs_known_places::find(&id).expect("saved").auto_reconnect);

    assert!(super::disconnect_with(&backend, &id).await);
    nfs_known_places::forget(&id);
}

/// A start-folder edit to a connected place rides its connection, and the live
/// volume answers the NEW folder in its params from then on: what the next
/// redial and every caller reading `params()` see, without waiting for a
/// reconnect.
#[tokio::test]
async fn a_changed_start_folder_on_an_open_place_moves_the_live_volume_params() {
    let backend = FakeNfs::answering([Answer::Resolves {
        export: "/",
        folder: None,
    }]);
    let id = id_of("start-folder.test", "/");
    let _ = connect(&backend, "", place("start-folder.test", "/"), "nfs-start-folder").await;
    let live = get_volume_manager().get(&id).expect("connected");
    let photos = std::path::Path::new(&*live.root().as_std_legacy()).join("photos");
    live.create_directory(&AppPath::legacy_unchecked(&photos))
        .await
        .expect("the fake holds the folder");

    let edit = KnownNfsPlace {
        start_folder: Some("/photos".to_string()),
        ..place("start-folder.test", "/")
    };
    assert_eq!(
        super::save_without_connecting_with(&backend, edit, "").await,
        SavedServerOutcome::Saved
    );

    assert_eq!(backend.dial_count(), 1, "❌ a start-folder change never redials");
    let live = get_volume_manager().get(&id).expect("still connected");
    assert_eq!(
        backend
            .params_of(&*live)
            .and_then(|params| params.start_folder)
            .as_deref(),
        Some("/photos"),
        "the live volume answers the folder the place now lands in"
    );
    assert_eq!(
        nfs_known_places::find(&id)
            .and_then(|saved| saved.start_folder)
            .as_deref(),
        Some("/photos")
    );

    assert!(super::disconnect_with(&backend, &id).await);
    nfs_known_places::forget(&id);
}

#[test]
fn a_server_id_ignores_which_export_it_was_asked_through() {
    let server = NfsServer::new("Server-Id.test", None);
    assert_eq!(
        nfs_known_places::server_id(&server),
        place("server-id.test", "/Media").server_id()
    );
}

/// The real crate, pointed at the fixture's relocated portmapper: the one
/// change the knfsd fixture needs (`NfsServer::portmap_port`), made here in the
/// test, ❌ never in the app. Everything else is [`super::CrateNfs`].
struct FixtureNfs;

impl NfsBackend for FixtureNfs {
    fn dial<'a>(
        &'a self,
        name: &'a str,
        mut params: NfsConnectionParams,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<Dialed, NfsConnectError>> + Send + 'a>> {
        params.server.portmap_port = cmdr_nfs::testing::fixture_server().portmap_port;
        super::CrateNfs.dial(name, params, cancel)
    }

    fn params_of(&self, volume: &dyn Volume) -> Option<NfsConnectionParams> {
        super::CrateNfs.params_of(volume)
    }

    fn sharing_connection(&self, volume: &dyn Volume, name: &str) -> Option<Arc<dyn Volume>> {
        super::CrateNfs.sharing_connection(volume, name)
    }

    fn set_auto_reconnect(&self, volume: &dyn Volume, on: bool) {
        super::CrateNfs.set_auto_reconnect(volume, on);
    }

    fn set_start_folder(&self, volume: &dyn Volume, folder: Option<String>) {
        super::CrateNfs.set_start_folder(volume, folder);
    }
}

#[tokio::test]
#[ignore = "needs the NFS fixture stack: apps/desktop/test/nfs-servers/start.sh (nfs-fixture)"]
async fn nfs_integration_a_typed_folder_connects_registers_the_export_and_saves_the_split() {
    let typed = KnownNfsPlace {
        host: "127.0.0.1".to_string(),
        port: None,
        export: "/insecure/photos/2024 summer".to_string(),
        uid: 1000,
        gid: 1000,
        other_gids: Vec::new(),
        start_folder: None,
        auto_reconnect: true,
        pinned: false,
        last_connected_at: String::new(),
    };

    let outcome = super::connect_and_register_with(
        &FixtureNfs,
        crate::network::connect_wiring::DialTicket::now(),
        "Fixture NAS",
        typed,
        "nfs-fixture-connect",
    )
    .await;

    let NfsConnection::Connected { volume_id } = outcome else {
        panic!("the knfsd fixture's /insecure must connect: {outcome:?}");
    };
    assert_eq!(
        volume_id,
        id_of("127.0.0.1", "/insecure"),
        "registered under the RESOLVED export"
    );
    let manager = get_volume_manager();
    let volume = manager.get(&volume_id).expect("a connect registers the volume it made");
    assert_eq!(volume.backend_kind(), BackendKind::Nfs);
    let entries = volume
        .list_directory(volume.root(), None)
        .await
        .expect("the export lists");
    assert!(
        entries.iter().any(|entry| entry.name == "hello.txt"),
        "the live volume, ❌ a stand-in"
    );
    let saved = nfs_known_places::find(&volume_id).expect("a connect remembers the place");
    assert_eq!(saved.export, "/insecure");
    assert_eq!(saved.start_folder.as_deref(), Some("/photos/2024 summer"));

    assert!(super::disconnect_with(&FixtureNfs, &volume_id).await);
    assert!(manager.get(&volume_id).is_none(), "a disconnect unregisters");
    nfs_known_places::forget(&volume_id);
}
