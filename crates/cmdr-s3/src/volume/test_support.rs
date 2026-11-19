//! A volume with no client behind it, for the cells that exercise the path
//! translation and the state machine without a server.

use std::sync::Arc;

use cmdr_fs::app_path::{AppPathBuf, RelativeAppPath};
use cmdr_fs::volume::Volume;
use cmdr_fs::volume::host::VolumeHost;
use cmdr_fs::volume::http_session::HttpSession;
use cmdr_fs::volume::remote_paths::RemoteRoot;
use url::Url;

use super::{S3Volume, S3VolumeInner};
use crate::params::{S3ConnectionParams, S3Provider};

/// The prefix the test volume mints, spelled out so a cell reads as the app
/// would see it.
pub(super) const PREFIX: &str = "s3://AKIATEST@127.0.0.1:1";

/// A place with no client: `bucket` is the place, `None` the account root.
pub(super) fn make_test_volume(bucket: Option<&str>) -> S3Volume {
    make_test_volume_with(bucket, VolumeHost::detached())
}

pub(super) fn make_test_volume_with(bucket: Option<&str>, host: VolumeHost) -> S3Volume {
    let provider = S3Provider::Other {
        endpoint: Url::parse("http://127.0.0.1:1").expect("a valid test URL"),
        region: None,
        path_style: true,
    };
    let params = S3ConnectionParams::new(provider, "AKIATEST", bucket).expect("a valid test provider");
    S3Volume {
        name: "test".to_string(),
        root: RemoteRoot::new(
            cmdr_fs::volume::s3_app_root(params.host(), params.port(), params.access_key_id()),
            std::path::Path::new(&params.remote_root()),
        ),
        inner: Arc::new_cyclic(|me| S3VolumeInner {
            session: HttpSession::new(me, "s3-test", &host, None, true),
            volume_id: "s3-test".to_string(),
            params: std::sync::RwLock::new(params),
            ledger: super::upload_ledger::UploadLedger::at(host.state_dir("s3")),
            host,
            written: std::sync::Mutex::new(std::collections::HashMap::new()),
            part_floor: std::sync::atomic::AtomicU64::new(crate::multipart::MIN_PART_SIZE),
            pause_hold_ms: std::sync::atomic::AtomicU64::new(5_000),
            beside_folders: std::sync::Mutex::new(std::collections::HashMap::new()),
        }),
    }
}

/// `rel` under `volume`'s root as a typed app path, the root itself for `""` or `"."`.
///
/// Panics on a `rel` that isn't a canonical relative tail (a `//`, a `.` or `..` name, a
/// leading or trailing `/`), so a cell can't silently address a different path.
pub(super) fn at(volume: &dyn Volume, rel: &str) -> AppPathBuf {
    let root = volume.root();
    if rel.is_empty() || rel == "." {
        return root.to_owned();
    }
    let tail = RelativeAppPath::new(rel).unwrap_or_else(|why| panic!("{rel:?} is no relative app path: {why}"));
    root.join_relative(tail)
        .unwrap_or_else(|why| panic!("{rel:?} doesn't join the root: {why}"))
}
