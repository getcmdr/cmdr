//! Which attached disk images are stored ON the drive being ejected.
//!
//! An image whose backing `.dmg` lives on the drive keeps that drive busy for as long
//! as it stays attached, and no amount of closing apps frees it: the image itself has
//! to be ejected first. That's its own sentence in a refusal, so it needs its own
//! [`HolderKind::DiskImage`](super::super::HolderKind::DiskImage).
//!
//! **The signal is `hdiutil info -plist`**, whose `hdid-pid` IS the process holding the
//! backing file open, so an image lands on a pid the holder walk already named rather
//! than as an invented one. ❌ Not IOKit's `IOHDIXHDDriveOutKernel` `image-path`
//! property, which names the image but carries no pid, and ❌ not that node's
//! `IOUserClientCreator`, which would mean parsing a sentence. (Verified on macOS 27.0,
//! 2026-09-16: a nested HFS+ image on an outer HFS+ image read `hdid-pid` 12984, which
//! is the pid `lsof` showed holding the outer volume's `inner.dmg`; `hdiutil info
//! -plist` answered in 18 ms.)
//!
//! ❗ **The server isn't always a process the walk can see.** `hdiutil attach` gets a
//! same-uid `diskimages-helper`, but an image Finder opens (DiskImages2) gets a
//! `diskimagesiod` of its own under uid 271, which `hdid-pid` still names and which
//! `CHECK_SAME_USER` hides from the walk. So [`unseen_on`] answers for those from this list
//! alone. (Verified on macOS 27.0.1, 2026-10-30: `open -g` and `diskutil image attach` of
//! an image on another image's volume, `diskutil unmount` of the outer volume answering
//! "dissented by PID 53397 (/usr/libexec/diskimagesiod)".)
//!
//! Whether an image sits on the drive is its backing file's `st_dev` against the target
//! roots' own — ❌ never a path prefix, which a firmlink or a symlinked `/Volumes` entry
//! breaks silently.

use std::path::{Path, PathBuf};

/// One attached disk image, reduced to what a refusal needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AttachedImage {
    /// The process serving it (`hdid-pid`), which is the one holding the backing file.
    pub(super) pid: u32,
    /// The backing FILE, which is what decides whether the image sits on the drive.
    pub(super) backing_file: PathBuf,
    /// What to call it: its mounted volume's name, else the backing file's own.
    pub(super) name: String,
}

/// The image `pid` serves, when its backing file sits on one of `targets`.
///
/// `device_of` is a parameter so the rule tests without a real image: it answers the
/// `st_dev` of a path, and a backing file nothing could read answers for nobody. ❗ That
/// last part is deliberate — "couldn't tell" must not become a `DiskImage`, which would
/// send a person off to eject an image that has nothing to do with this drive.
pub(super) fn stored_on<'a>(
    images: &'a [AttachedImage],
    pid: u32,
    targets: &[u64],
    device_of: impl Fn(&Path) -> Option<u64>,
) -> Option<&'a AttachedImage> {
    images
        .iter()
        .filter(|image| image.pid == pid)
        .find(|image| sits_on(image, targets, &device_of))
}

/// The images on `targets` that no pid in `walked` serves, one per serving process.
///
/// This is how an image reaches a refusal when the walk can't see its server at all:
/// `diskimagesiod` runs under its own uid. Holding the drive needs no further proof, since
/// an attached image keeps the volume its backing file sits on busy by construction. The
/// `device_of` and "couldn't tell" rules are [`stored_on`]'s.
pub(super) fn unseen_on<'a>(
    images: &'a [AttachedImage],
    walked: &[u32],
    targets: &[u64],
    device_of: impl Fn(&Path) -> Option<u64>,
) -> Vec<&'a AttachedImage> {
    let mut unseen: Vec<&AttachedImage> = Vec::new();
    for image in images {
        let already_named = walked.contains(&image.pid) || unseen.iter().any(|seen| seen.pid == image.pid);
        if !already_named && sits_on(image, targets, &device_of) {
            unseen.push(image);
        }
    }
    unseen
}

/// Whether `image`'s backing file sits on one of `targets`, by its device.
fn sits_on(image: &AttachedImage, targets: &[u64], device_of: &impl Fn(&Path) -> Option<u64>) -> bool {
    device_of(&image.backing_file).is_some_and(|device| targets.contains(&device))
}

/// Every image attached on this machine, `None` when nothing could be read.
///
/// ❗ `None` means "couldn't tell", which the caller reads as "no image answers for this
/// pid" rather than as "no images are attached". An unreadable answer costs a better
/// sentence, never a wrong one.
#[cfg(target_os = "macos")]
pub(super) use macos::attached;

/// Linux and Windows attach no disk images Cmdr reads, and their holder walk names nobody to ask
/// about.
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(super) fn attached() -> Option<Vec<AttachedImage>> {
    None
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::PathBuf;

    use serde::Deserialize;

    use super::AttachedImage;

    /// Every image attached on this machine, `None` when nothing could be read.
    ///
    /// ❗ Runs on the holder scan's own abandonable thread, at most once per refusal. The
    /// scan's budget IS its timeout: nobody joins that thread, so a wedged `hdiutil` costs
    /// the answer and never the eject.
    pub(in crate::file_system::volume::eject::holders::facts) fn attached() -> Option<Vec<AttachedImage>> {
        Some(
            list()?
                .images
                .into_iter()
                .filter_map(|image| {
                    Some(AttachedImage {
                        // An image nothing serves has no pid to name, and a made-up one
                        // would collide with a real process.
                        pid: image.pid?,
                        name: name_of(&image),
                        backing_file: image.backing_file,
                    })
                })
                .collect(),
        )
    }

    /// `hdiutil info -plist`, parsed.
    fn list() -> Option<AttachedList> {
        let answered = match std::process::Command::new("hdiutil").args(["info", "-plist"]).output() {
            Ok(answered) if answered.status.success() => answered,
            Ok(answered) => {
                log::debug!(
                    target: "eject",
                    "`hdiutil info` exited with {:?}, so no disk image can answer for a holder",
                    answered.status.code()
                );
                return None;
            }
            Err(error) => {
                log::debug!(target: "eject", "`hdiutil info` couldn't start, so no disk image can answer for a holder: {error}");
                return None;
            }
        };
        plist::from_bytes(&answered.stdout)
            .inspect_err(|error| {
                log::debug!(target: "eject", "`hdiutil info` answered a plist nothing could read: {error}");
            })
            .ok()
    }

    /// What to call an image: its mounted volume's name, else the backing file's own.
    ///
    /// Only the log and the MCP reply read it; the toast's disk-image sentence names no
    /// image, since "eject that image first" is the same advice whatever it's called.
    fn name_of(image: &ImageEntry) -> String {
        image
            .entities
            .iter()
            .filter_map(|entity| entity.mount_point.as_ref())
            .find_map(|mount| mount.file_name())
            .or_else(|| image.backing_file.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// `hdiutil info -plist`: every image attached on the machine, ours or not.
    #[derive(Debug, Deserialize)]
    struct AttachedList {
        #[serde(default)]
        images: Vec<ImageEntry>,
    }

    /// One image in `hdiutil info -plist`.
    #[derive(Debug, Deserialize)]
    struct ImageEntry {
        /// The process serving it. Absent for an image no `hdid` backs.
        #[serde(rename = "hdid-pid")]
        pid: Option<u32>,
        /// The backing file, spelled exactly as it was passed to `hdiutil attach`.
        #[serde(rename = "image-path")]
        backing_file: PathBuf,
        #[serde(rename = "system-entities", default)]
        entities: Vec<SystemEntity>,
    }

    /// One node of an image.
    #[derive(Debug, Deserialize)]
    struct SystemEntity {
        /// Absent when the node isn't mounted.
        #[serde(rename = "mount-point")]
        mount_point: Option<PathBuf>,
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// What `hdiutil info -plist` answers with an image attached whose backing file
        /// sits on another image's volume (recorded on macOS 27.0, 2026-09-16).
        const NESTED: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>images</key>
  <array>
    <dict>
      <key>hdid-pid</key><integer>12624</integer>
      <key>image-path</key><string>/private/tmp/outer.dmg</string>
      <key>system-entities</key>
      <array>
        <dict><key>dev-entry</key><string>/dev/disk4</string></dict>
        <dict>
          <key>dev-entry</key><string>/dev/disk4s1</string>
          <key>mount-point</key><string>/Volumes/CMDRPROBEOUT</string>
        </dict>
      </array>
    </dict>
    <dict>
      <key>hdid-pid</key><integer>12984</integer>
      <key>image-path</key><string>/Volumes/CMDRPROBEOUT/inner.dmg</string>
      <key>system-entities</key>
      <array>
        <dict>
          <key>dev-entry</key><string>/dev/disk5s1</string>
          <key>mount-point</key><string>/Volumes/CMDRPROBEIN</string>
        </dict>
      </array>
    </dict>
  </array>
</dict>
</plist>"#;

        fn parsed(answer: &[u8]) -> AttachedList {
            plist::from_bytes(answer).expect("the recorded answer parses")
        }

        #[test]
        fn a_recorded_answer_reads_the_serving_pid_the_backing_file_and_the_volume_name() {
            let listed = parsed(NESTED);
            let read: Vec<(Option<u32>, String, String)> = listed
                .images
                .iter()
                .map(|image| {
                    (
                        image.pid,
                        image.backing_file.to_string_lossy().into_owned(),
                        name_of(image),
                    )
                })
                .collect();
            assert_eq!(
                read,
                [
                    (
                        Some(12624),
                        "/private/tmp/outer.dmg".to_string(),
                        "CMDRPROBEOUT".to_string()
                    ),
                    (
                        Some(12984),
                        "/Volumes/CMDRPROBEOUT/inner.dmg".to_string(),
                        "CMDRPROBEIN".to_string()
                    ),
                ],
                "the volume a person would eject, not the `.dmg` behind it"
            );
        }

        #[test]
        fn an_image_with_no_serving_process_is_left_out_rather_than_given_a_made_up_pid() {
            let listed = parsed(
                br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>images</key><array>
  <dict><key>image-path</key><string>/private/tmp/orphan.dmg</string></dict>
</array></dict></plist>"#,
            );
            assert_eq!(listed.images.len(), 1, "the image is listed");
            assert_eq!(listed.images[0].pid, None, "but nothing serves it");
            assert_eq!(
                name_of(&listed.images[0]),
                "orphan.dmg",
                "an image nothing mounted falls back to its file's name"
            );
        }

        #[test]
        fn an_answer_with_no_images_at_all_parses_as_an_empty_list() {
            // What an idle Mac answers.
            let listed = parsed(
                br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>images</key><array/></dict></plist>"#,
            );
            assert!(listed.images.is_empty());
        }

        /// An image opened from Finder: `diskimagesiod` under its own uid 271, whose pid
        /// `hdid-pid` still names (recorded on macOS 27.0.1, 2026-10-30).
        #[test]
        fn an_image_served_by_the_daemon_still_names_its_pid() {
            let listed = parsed(
                br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>images</key><array>
  <dict>
    <key>diskimages2</key><true/>
    <key>hdid-pid</key><integer>53397</integer>
    <key>image-path</key><string>/Volumes/CMDRDIOUT/inner.dmg</string>
    <key>owner-uid</key><integer>501</integer>
    <key>system-entities</key>
    <array>
      <dict><key>dev-entry</key><string>/dev/disk7</string></dict>
      <dict>
        <key>dev-entry</key><string>/dev/disk7s1</string>
        <key>mount-point</key><string>/Volumes/CMDRDIIN</string>
      </dict>
    </array>
  </dict>
</array></dict></plist>"#,
            );
            assert_eq!(listed.images[0].pid, Some(53397));
            assert_eq!(name_of(&listed.images[0]), "CMDRDIIN");
        }

        /// The real tool, on whatever this Mac has attached: the parse has to survive
        /// every key a live `hdiutil` adds, and the call has to be cheap enough to sit
        /// inside a 1.5 s budget.
        #[test]
        fn the_real_tool_answers_something_this_parses() {
            let listed = attached().expect("`hdiutil info -plist` answers on a Mac");
            for image in &listed {
                assert!(image.pid > 0, "every listed image names the process serving it");
                assert!(
                    image.backing_file.is_absolute(),
                    "and the backing file it was attached from, got {:?}",
                    image.backing_file
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(pid: u32, backing_file: &str, name: &str) -> AttachedImage {
        AttachedImage {
            pid,
            backing_file: PathBuf::from(backing_file),
            name: name.to_string(),
        }
    }

    /// Two images: the outer one backed by a file on the Mac (device 1), the nested one
    /// by a file on the outer one's volume (device 16).
    fn nested() -> Vec<AttachedImage> {
        vec![
            image(12624, "/private/tmp/outer.dmg", "CMDRPROBEOUT"),
            image(12984, "/Volumes/CMDRPROBEOUT/inner.dmg", "CMDRPROBEIN"),
        ]
    }

    fn device_of(path: &Path) -> Option<u64> {
        Some(if path.starts_with("/Volumes/CMDRPROBEOUT") {
            16
        } else {
            1
        })
    }

    #[test]
    fn an_image_whose_backing_file_sits_on_the_drive_answers_for_its_own_pid() {
        let images = nested();
        let found = stored_on(&images, 12984, &[16], device_of).expect("the nested image answers");
        assert_eq!(found.name, "CMDRPROBEIN");
    }

    #[test]
    fn an_image_stored_somewhere_else_answers_for_nobody() {
        // The outer image's own backing file is on the Mac, so ejecting this drive has
        // nothing to do with it and naming it would send a person after the wrong thing.
        assert_eq!(stored_on(&nested(), 12624, &[16], device_of), None);
    }

    #[test]
    fn a_pid_no_image_belongs_to_answers_for_nobody() {
        assert_eq!(stored_on(&nested(), 983, &[16], device_of), None);
    }

    #[test]
    fn a_backing_file_that_wouldnt_stat_answers_for_nobody() {
        // ❗ A device nobody could read is "couldn't tell", ❌ never "it's on the drive".
        assert_eq!(stored_on(&nested(), 12984, &[16], |_| None), None);
    }

    #[test]
    fn an_image_the_walk_never_named_answers_for_itself() {
        let images = nested();
        let unseen = unseen_on(&images, &[], &[16], device_of);
        assert_eq!(unseen, [&images[1]], "the image on the drive, and not the one off it");
    }

    #[test]
    fn an_image_the_walk_already_named_isnt_added_twice() {
        // A same-uid helper reached the classifier by its pid, and rule 3 answered there.
        assert!(unseen_on(&nested(), &[12984], &[16], device_of).is_empty());
    }

    #[test]
    fn one_daemon_serving_two_images_on_the_drive_is_one_holder() {
        // The walk and the scan both dedupe by pid, and so does this.
        let images = [
            image(53397, "/Volumes/CMDRPROBEOUT/one.dmg", "ONE"),
            image(53397, "/Volumes/CMDRPROBEOUT/two.dmg", "TWO"),
        ];
        assert_eq!(unseen_on(&images, &[], &[16], device_of), [&images[0]]);
    }

    #[test]
    fn an_unseen_image_whose_backing_file_wouldnt_stat_answers_for_nobody() {
        // ❗ Same rule as `stored_on`: "couldn't tell" must not become a `DiskImage`.
        assert!(unseen_on(&nested(), &[], &[16], |_| None).is_empty());
    }

    #[test]
    fn a_drive_with_several_mounts_matches_any_of_their_devices() {
        // Every mount of the disk being torn down counts: an image stored on a SIBLING
        // partition keeps the whole disk up just as surely.
        let images = nested();
        assert!(stored_on(&images, 12984, &[4, 16, 9], device_of).is_some());
        assert_eq!(stored_on(&images, 12984, &[4, 9], device_of), None);
    }
}
