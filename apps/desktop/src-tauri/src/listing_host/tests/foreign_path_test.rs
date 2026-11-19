//! A pane path the volume stores another way, where the app's half decides it:
//! the SMB upgrade swaps a share's backend in the app's volume registry while a
//! listing on it is still reading. The engine's half (`list_as_stored`, the
//! respell) is `crates/cmdr-listing/src/foreign_path_test.rs`, against a fake
//! registry.

use cmdr_fs::app_path::AppPath;
use std::future::Future;
use std::path::Path;

use cmdr_fs::entry::FileEntry;
use cmdr_fs::volume::{InMemoryVolume, ListingProgress, Volume, VolumeError};
use cmdr_listing::testing::SpelledVolume;

/// `fotók` composed (how the server stores it) and decomposed (how the macOS
/// kernel mount spelled it for the pane).
const STORED: &str = "/fot\u{f3}k";
const FOREIGN: &str = "/foto\u{301}k";

async fn album() -> InMemoryVolume {
    let volume = InMemoryVolume::new("Share");
    volume
        .create_directory(&AppPath::legacy_unchecked(Path::new(STORED)))
        .await
        .expect("seed the album");
    volume
        .create_file(
            &AppPath::legacy_unchecked(&Path::new(STORED).join("photo.jpg")),
            b"jpeg",
        )
        .await
        .expect("seed a photo");
    volume
}

/// The kernel mount, reading a folder while the direct connection replaces it:
/// the swap lands after this listing started and before it's cached, which is
/// exactly when a pane landing on the share triggers the upgrade
/// (`network::smb_pane_upgrade`).
struct SwappedMidListing {
    kernel: InMemoryVolume,
    volume_id: String,
    successor: std::sync::Mutex<Option<std::sync::Arc<dyn Volume>>>,
}

impl Volume for SwappedMidListing {
    fn name(&self) -> &str {
        self.kernel.name()
    }

    fn root(&self) -> &AppPath {
        self.kernel.root()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn list_directory<'a>(
        &'a self,
        path: &'a AppPath,
        on_progress: Option<&'a (dyn Fn(ListingProgress) + Sync)>,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        Box::pin(async move {
            let successor = self.successor.lock().expect("test lock").take();
            if let Some(successor) = successor {
                crate::network::smb_upgrade::register_replacing_predecessor(&self.volume_id, successor).await;
            }
            self.kernel.list_directory(path, on_progress).await
        })
    }

    fn get_metadata<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<FileEntry, VolumeError>> + Send + 'a>> {
        self.kernel.get_metadata(path)
    }

    fn exists<'a>(&'a self, path: &'a AppPath) -> std::pin::Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        self.kernel.exists(path)
    }

    fn is_directory<'a>(
        &'a self,
        path: &'a AppPath,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<bool, VolumeError>> + Send + 'a>> {
        self.kernel.is_directory(path)
    }
}

/// The upgrade's own re-read runs before this listing is cached, so it can't
/// see it: the listing has to notice on its own that the backend that read it
/// is gone, or the pane keeps the kernel's spelling on a byte-exact share.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_listing_read_while_the_backend_is_replaced_lands_on_the_stored_spelling() {
    use crate::file_system::volume::manager::get_volume_manager;
    use crate::listing_host::listings;
    use crate::test_support::wait_until_async;
    use cmdr_fs::testing::unique_test_id;
    use cmdr_listing::testing::{TestListingGuard, read_to_end, request};

    let volume_id = format!("respell-mid-listing-{}", uuid::Uuid::new_v4());
    let kernel = InMemoryVolume::new("Share");
    kernel
        .create_directory(&AppPath::legacy_unchecked(Path::new(FOREIGN)))
        .await
        .expect("seed");
    kernel
        .create_file(
            &AppPath::legacy_unchecked(&Path::new(FOREIGN).join("photo.jpg")),
            b"jpeg",
        )
        .await
        .expect("seed");
    let direct = SpelledVolume::new(album().await).resolving(FOREIGN, Ok(Some(STORED)));
    let swapped = SwappedMidListing {
        kernel,
        volume_id: volume_id.clone(),
        successor: std::sync::Mutex::new(Some(std::sync::Arc::new(direct))),
    };
    get_volume_manager().register(&volume_id, std::sync::Arc::new(swapped));
    let listing = TestListingGuard::adopt(listings(), unique_test_id("respell-mid-listing"));

    read_to_end(listings(), &request(listing.id(), &volume_id, FOREIGN))
        .await
        .expect("the kernel mount lists the folder");
    let respelled = || {
        listing
            .engine()
            .location(&listing.listing_id())
            .is_some_and(|(_, path)| path == Path::new(STORED))
    };
    wait_until_async(
        std::time::Duration::from_secs(5),
        "the listing to be respelled",
        respelled,
    )
    .await;
    get_volume_manager().unregister(&volume_id);

    let paths: Vec<String> = listing.entries().into_iter().map(|e| e.path).collect();
    assert_eq!(
        paths,
        vec![format!("{STORED}/photo.jpg")],
        "entries carry the stored bytes"
    );
}
