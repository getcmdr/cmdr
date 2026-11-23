//! The whole bundle as one text file the File viewer opens: the error-report and feedback
//! dialogs' "see everything" link.
//!
//! - **Rendered from the zip, not from the pipeline's inputs**, so the file shows the bundle
//!   the upload would carry, cap and redaction included, rather than a re-telling of it.
//! - **A real file, not in-memory content.** The bundle can be megabytes of text, and the viewer's
//!   large-file backends, search, and window title all work off a path.
//! - **One dir per instance** (`<app_data_dir>/report-preview`, beside the other per-instance
//!   temps), one `<ERR-id>.txt` per report: a second look at the same report overwrites it,
//!   and startup clears the dir. The text is the redacted bundle, owner-only on disk.

use crate::error_reporter::{self, BundleRequest, FLOW_A_BUNDLE_CAP_MB};
use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, RwLock};

use crate::ignore_poison::RwLockIgnorePoison;

/// The per-instance dir, set at startup by [`init_report_preview_dir`].
static PREVIEW_DIR: LazyLock<RwLock<Option<PathBuf>>> = LazyLock::new(|| RwLock::new(None));

/// A preview written to disk: where it is, and the report id it was built under.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportPreviewFile {
    pub path: String,
    /// The bundle's id. Pass it back to the send, so the redaction context (keyed by id) and the
    /// report's name match what the person looked at.
    pub id: String,
}

/// Why a preview file couldn't be made. `detail` is for the log, never the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ReportPreviewError {
    /// The bundle couldn't be built from the logs, or didn't read back as a zip.
    BundleUnavailable { detail: String },
    /// The app data dir is unknown, or the file couldn't be written there.
    NotWritten { detail: String },
}

impl std::fmt::Display for ReportPreviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BundleUnavailable { detail } => write!(f, "couldn't build the bundle: {detail}"),
            Self::NotWritten { detail } => write!(f, "couldn't write the preview: {detail}"),
        }
    }
}

/// Records the per-instance dir and clears what an earlier run left in it. Called once at
/// startup from `lib.rs`. The clear runs on its own thread, off the startup path.
pub fn init_report_preview_dir(dir: PathBuf) {
    *PREVIEW_DIR.write_ignore_poison() = Some(dir.clone());
    let spawned = std::thread::Builder::new()
        .name("report-preview-reaper".to_string())
        .spawn(move || clear_preview_dir(&dir));
    if let Err(e) = spawned {
        log::warn!(target: "cmdr_lib::error_reporter", "Couldn't start the report preview reaper: {e}");
    }
}

/// Build the bundle `request` describes, Flow A cap included, and write it as text for the viewer.
pub async fn write_preview<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    request: BundleRequest,
) -> Result<ReportPreviewFile, ReportPreviewError> {
    let dir = PREVIEW_DIR
        .read_ignore_poison()
        .clone()
        .ok_or_else(|| ReportPreviewError::NotWritten {
            detail: "no app data dir".to_string(),
        })?;
    let bundle = error_reporter::build_bundle(app, request)
        .await
        .map_err(|detail| ReportPreviewError::BundleUnavailable { detail })?;
    let capped = error_reporter::cap_bundle_to_mb(bundle.zip_bytes, FLOW_A_BUNDLE_CAP_MB);
    let text = bundle_as_text(&capped).map_err(|detail| ReportPreviewError::BundleUnavailable { detail })?;
    // `bundle.id` is the vetted one (`resolve_bundle_id`), so it's safe as a filename.
    let path = write_preview_file(&dir, &bundle.id, &text)
        .map_err(|e| ReportPreviewError::NotWritten { detail: e.to_string() })?;
    Ok(ReportPreviewFile {
        path: path.display().to_string(),
        id: bundle.id,
    })
}

/// Every entry of a bundle zip, in bundle order, each under a `==> name <==` header (the shape
/// `tail` gives several files). Entries are separated by a blank line.
pub fn bundle_as_text(zip_bytes: &[u8]) -> Result<String, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).map_err(|e| format!("open zip: {e}"))?;
    let mut sections = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| format!("zip entry {index}: {e}"))?;
        let name = entry
            .name()
            .map_err(|e| format!("zip entry {index} name: {e}"))?
            .into_owned();
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|e| format!("read zip entry {index}: {e}"))?;
        let mut content = String::from_utf8_lossy(&bytes).into_owned();
        if !content.ends_with('\n') {
            content.push('\n');
        }
        sections.push(format!("==> {name} <==\n{content}"));
    }
    Ok(sections.join("\n"))
}

/// Writes `text` to `<dir>/<id>.txt`, owner-only (`cmdr_fs::private_file`), through a temp and a
/// rename: a viewer already showing an earlier look at this report never reads a half-written
/// file.
pub fn write_preview_file(dir: &Path, id: &str, text: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{id}.txt"));
    cmdr_fs::private_file::write_atomically(&path, text.as_bytes())?;
    Ok(path)
}

/// Removes the preview dir and everything in it. A missing dir is already clear.
pub fn clear_preview_dir(dir: &Path) {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => log::warn!(target: "cmdr_lib::error_reporter", "Couldn't clear old report previews: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    use std::io::Write;
    use zip::write::{SimpleFileOptions, ZipWriter};

    fn zip_of(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut writer = ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, content) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .expect("start entry");
            writer.write_all(content.as_bytes()).expect("write entry");
        }
        writer.finish().expect("finish zip").into_inner()
    }

    #[test]
    fn renders_every_entry_in_bundle_order_under_its_name() {
        let zip = zip_of(&[
            ("manifest.json", "{\n  \"id\": \"ERR-AB23X\"\n}"),
            ("logs/cmdr.log", "line one\nline two\n"),
            ("logs/cmdr.1.log", "older line\n"),
        ]);

        let text = bundle_as_text(&zip).expect("a well-formed bundle renders");

        assert_eq!(
            text,
            "==> manifest.json <==\n{\n  \"id\": \"ERR-AB23X\"\n}\n\n\
             ==> logs/cmdr.log <==\nline one\nline two\n\n\
             ==> logs/cmdr.1.log <==\nolder line\n"
        );
    }

    #[test]
    fn refuses_bytes_that_are_not_a_zip() {
        assert!(bundle_as_text(b"not a zip").is_err());
    }

    #[test]
    fn writes_the_preview_under_the_report_id_and_overwrites_it_on_the_next_look() {
        let dir = TestDir::new("report_preview_write");
        let preview_dir = dir.join("report-preview");

        let first = write_preview_file(&preview_dir, "ERR-AB23X", "first").expect("first write");
        let second = write_preview_file(&preview_dir, "ERR-AB23X", "second").expect("second write");

        assert_eq!(first, preview_dir.join("ERR-AB23X.txt"));
        assert_eq!(first, second);
        assert_eq!(std::fs::read_to_string(&second).expect("read back"), "second");
        // Only the preview itself: the atomic write's temp is gone.
        assert_eq!(std::fs::read_dir(&preview_dir).expect("list").count(), 1);
    }

    #[test]
    fn the_preview_is_readable_by_its_owner_only() {
        let dir = TestDir::new("report_preview_mode");
        let path = write_preview_file(&dir.join("report-preview"), "ERR-AB23X", "logs").expect("write");
        assert!(cmdr_fs::private_file::is_private_to_current_user(&path).expect("read its access"));
        #[cfg(unix)] // POSIX modes.
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).expect("stat").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn clearing_takes_every_preview_and_tolerates_a_missing_dir() {
        let dir = TestDir::new("report_preview_clear");
        let preview_dir = dir.join("report-preview");
        clear_preview_dir(&preview_dir);

        write_preview_file(&preview_dir, "ERR-AB23X", "a").expect("write a");
        write_preview_file(&preview_dir, "ERR-CD45Y", "b").expect("write b");
        clear_preview_dir(&preview_dir);

        assert!(!preview_dir.exists());
    }
}
