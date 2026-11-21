//! `destination_exists`: the transfer and compress dialogs' "is this destination
//! already there?", which counts a name the volume holds in another Unicode
//! spelling, beside `path_exists`, which stays byte-exact.
//!
//! `InMemoryVolume` matches names byte for byte, like an SMB share. Real Samba:
//! `operations_host/backend_suites/smb_look_alike_test.rs`.

use cmdr_fs::app_path;
use cmdr_fs::app_path::AppPath;
use std::sync::Arc;

use crate::file_system::volume::manager::get_volume_manager;
use crate::file_system::volume::{InMemoryVolume, Volume};

use super::{destination_exists, path_exists};

const CAFE_NFC: &str = "caf\u{e9}.zip";
const CAFE_NFD: &str = "cafe\u{301}.zip";

/// Registers a byte-exact volume holding `/share` and one entry per `names`
/// inside it, and hands back its id. Unregister when done.
async fn share_holding(names: &[&str]) -> String {
    let id = format!("destination-exists-{}", uuid::Uuid::new_v4());
    let volume = InMemoryVolume::new("Share");
    volume.create_directory(app_path!("/share")).await.unwrap();
    for name in names {
        volume
            .create_file(
                AppPath::from_normalized(&format!("/share/{name}")).expect("canonical"),
                b"x",
            )
            .await
            .unwrap();
    }
    get_volume_manager().register(&id, Arc::new(volume) as Arc<dyn Volume>);
    id
}

async fn destination_answer(id: &str, name: &str) -> cmdr_fs::deadline::TimedOut<bool> {
    destination_exists(Some(id.to_string()), format!("/share/{name}")).await
}

#[tokio::test]
async fn a_name_the_volume_holds_in_another_spelling_is_there() {
    for (stored, asked) in [(CAFE_NFC, CAFE_NFD), (CAFE_NFD, CAFE_NFC)] {
        let id = share_holding(&[stored]).await;
        let answer = destination_answer(&id, asked).await;
        get_volume_manager().unregister(&id);
        assert!(
            answer.data && !answer.timed_out,
            "{stored:?} is what the user sees as {asked:?}; got {answer:?}"
        );
    }
}

/// The copy/move dialog asks about a FOLDER: one the share holds in the other
/// spelling is where the copy merges, so "this folder will be created" would lie.
#[tokio::test]
async fn a_folder_the_volume_holds_in_another_spelling_is_there() {
    let id = share_holding(&[]).await;
    let volume = get_volume_manager().get(&id).unwrap();
    volume.create_directory(app_path!("/share/fot\u{f3}k")).await.unwrap();
    let answer = destination_exists(Some(id.clone()), "/share/foto\u{301}k".to_string()).await;
    get_volume_manager().unregister(&id);
    assert!(answer.data && !answer.timed_out, "{answer:?}");
}

/// Two look-alikes, neither spelled as asked: the write refuses rather than
/// guess, but the dialog still hears "there", ❌ never "will be created".
#[tokio::test]
async fn a_name_two_stored_spellings_fit_is_there() {
    let id = share_holding(&["\u{e9}l\u{151}.zip", "e\u{301}lo\u{30b}.zip"]).await;
    let answer = destination_answer(&id, "\u{e9}lo\u{30b}.zip").await;
    get_volume_manager().unregister(&id);
    assert!(answer.data && !answer.timed_out, "{answer:?}");
}

#[tokio::test]
async fn a_name_held_in_no_spelling_is_not_there() {
    let id = share_holding(&[CAFE_NFC]).await;
    let exact = destination_answer(&id, CAFE_NFC).await;
    let absent = destination_answer(&id, "caf\u{e9} 2.zip").await;
    get_volume_manager().unregister(&id);
    assert!(exact.data && !exact.timed_out, "{exact:?}");
    assert!(!absent.data && !absent.timed_out, "{absent:?}");
}

/// `path_exists` keeps meaning these exact bytes: a pane's remembered path and
/// the directory-eviction poll ask it about a path they then list.
#[tokio::test]
async fn path_exists_stays_byte_exact() {
    let id = share_holding(&[CAFE_NFC]).await;
    let answer = path_exists(Some(id.clone()), format!("/share/{CAFE_NFD}")).await;
    get_volume_manager().unregister(&id);
    assert!(!answer.data && !answer.timed_out, "{answer:?}");
}
