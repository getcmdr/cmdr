//! How the planning walk spells the children it goes on with: the scan walk's
//! rule (`cmdr_fs::app_path::ListedFolder`), pinned at the plan.

use cmdr_fs::app_path;
use std::future::Future;
use std::sync::Mutex;

use super::*;
use crate::testing::TestOperations;
use crate::transfer::volume::forward_volume_methods;
use cmdr_fs::volume::InMemoryVolume;

/// A source that records every folder the walk lists, and lists one name as
/// `lie` says (a broken or hostile server's listing).
struct RecordingListing {
    inner: InMemoryVolume,
    listed: Mutex<Vec<String>>,
    lie: Option<(&'static str, &'static str)>,
}

impl Volume for RecordingListing {
    forward_volume_methods!(inner => name, root, exists, is_directory, get_metadata, get_space_info);

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn list_directory<'a>(
        &'a self,
        path: &'a AppPath,
        on_progress: Option<&'a (dyn Fn(cmdr_fs::volume::ListingProgress) + Sync)>,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Vec<FileEntry>, VolumeError>> + Send + 'a>> {
        Box::pin(async move {
            self.listed.lock().unwrap().push(path.as_str().to_string());
            let mut entries = self.inner.list_directory(path, on_progress).await?;
            if let Some((real, listed_as)) = self.lie {
                for entry in &mut entries {
                    if entry.name == real {
                        entry.name = listed_as.to_string();
                    }
                }
            }
            Ok(entries)
        })
    }
}

async fn plan_of(source: RecordingListing, top: &str) -> (Arc<RecordingListing>, Result<FreshPlan, EditError>) {
    let engine = TestOperations::new();
    let source = Arc::new(source);
    let as_volume: Arc<dyn Volume> = Arc::clone(&source) as Arc<dyn Volume>;
    let state = Arc::new(engine.new_operation_state(Duration::ZERO));
    let plan = plan_sources_with_context(
        &as_volume,
        &[PathBuf::from(top)],
        ConflictResolution::Stop,
        &state,
        None,
        None,
    )
    .await;
    (source, plan)
}

/// ❗ A listed name that isn't one plain name fails the plan, ❌ never a join:
/// `Path::join` of `a/b` packed a different file under the folder's name, and
/// of `..` re-listed the parent.
#[tokio::test]
async fn a_listed_name_that_is_no_plain_name_fails_the_plan() {
    let inner = InMemoryVolume::new("remote");
    inner.create_directory(app_path!("/src")).await.expect("mkdir");
    inner.create_file(app_path!("/src/x.txt"), b"x").await.expect("seed");
    let source = RecordingListing {
        inner,
        listed: Mutex::default(),
        lie: Some(("x.txt", "a/b")),
    };

    let (_, plan) = plan_of(source, "/src").await;

    assert!(
        matches!(plan, Err(EditError::Op(WriteOperationError::ReadError { .. }))),
        "the plan refuses the folder that listed it"
    );
}

/// A selection spelled with a doubled `/` lists its own children by their
/// canonical path, the spelling every other walk hands a volume.
#[tokio::test]
async fn a_doubled_separator_in_a_selection_walks_its_children_canonically() {
    let inner = InMemoryVolume::new("remote");
    for dir in [app_path!("/src"), app_path!("/src/sub"), app_path!("/src/sub/d")] {
        inner.create_directory(dir).await.expect("mkdir");
    }
    inner
        .create_file(app_path!("/src/sub/d/f.txt"), b"x")
        .await
        .expect("seed");
    let source = RecordingListing {
        inner,
        listed: Mutex::default(),
        lie: None,
    };

    let (source, plan) = plan_of(source, "/src//sub").await;

    assert!(plan.is_ok(), "the plan packs the folder");
    assert_eq!(*source.listed.lock().unwrap(), ["/src//sub", "/src/sub/d"]);
}
