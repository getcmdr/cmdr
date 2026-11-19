//! "Tell me about this" over the demo's volume: `about_start`, `about_cancel`,
//! and `about_stop_verdict`, answered the way the desktop's host answers them
//! (`apps/desktop/src-tauri/src/about/`): the same plan (`cmdr_about::card::plan`),
//! the same facts, and the same event order on the card's channel.
//!
//! What the demo's volume can answer is the item's identity (name, size, dates)
//! and its real type (the sniff of its first and last bytes against its
//! extension). A browser has no OS records (downloads, Spotlight, code
//! signatures, default apps, owners), no drive index, and no repo, so the plan
//! asks none of them, and the card's volume note says Cmdr sees only some of it.
//! No AI either: the verdict is `aiOff`, as on a desktop with AI off.
//!
//! The channel: a `Channel` argument crosses as its `__CHANNEL__:…` string, and
//! every message goes to the page as a [`CHANNEL_EVENT`] notice carrying that
//! string, which the page hands to the frame that made the channel
//! (`apps/desktop/src/lib/demo/webview.ts`). A card's last message is the
//! channel's end, as Tauri sends one when the Rust side drops it.

use cmdr_fs::app_path::AppPath;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use cmdr_about::actions::{ActionCaps, Actions, RemoveBy, actions};
use cmdr_about::card::{AboutCard, AboutFact, AboutPlan, Applied, OsCaps, PlanInput, VolumeCaps, plan};
use cmdr_about::fact::{AboutTarget, Fact, FactKind, UnknownWhy};
use cmdr_about::headline::{Headline, headline};
use cmdr_about::owner::OwnerQuestion;
use cmdr_about::payload::Identity;
use cmdr_about::removal::{Removal, consequences};
use cmdr_about::sniff::{HEAD_BYTES, RealType, TAIL_BYTES, real_type};
use cmdr_about::text::{ItemText, hidden_in_name};
use cmdr_fs::entry::FileEntry;
use cmdr_fs::ignore_poison::IgnorePoison as _;
use cmdr_fs::volume::{Volume, VolumeError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::events::EventSink;
use crate::invoke::parse;

/// Every command [`DemoAbout::invoke`] answers, by its Tauri name.
pub const ABOUT_COMMANDS: &[&str] = &["about_start", "about_cancel", "about_stop_verdict"];

/// The page's notice for one message on a Tauri `Channel`: `{ channel, message }`,
/// or `{ channel, end: true }` once nothing more follows. Not a Tauri event: the
/// page's backend routes it to the frame that made the channel.
pub const CHANNEL_EVENT: &str = "demo://channel";

/// The rows the demo's volume answers. Everything else needs the OS, an index,
/// or a repo, none of which a browser tab has.
const ANSWERED: [FactKind; 2] = [FactKind::Identity, FactKind::RealType];

/// What a browser's OS keeps about any item: nothing Cmdr can read.
const NO_OS_RECORDS: OsCaps = OsCaps {
    download_records: false,
    use_history: false,
    code_signing: false,
    default_app: false,
    install_receipts: false,
};

/// The cards over one volume, and the cancels that arrived for them.
pub struct DemoAbout {
    volume: Arc<dyn Volume>,
    sink: Arc<dyn EventSink>,
    /// Cards cancelled while gathering, or before their start arrived (a tombstone,
    /// as the desktop's registry keeps one, so a start after its cancel is a no-op).
    /// A card's own end clears its id; a cancel after that end leaves one id behind,
    /// a few bytes per closed card, gone on reload.
    cancelled: Mutex<HashSet<String>>,
}

impl DemoAbout {
    /// The cards over `volume`, sending each card's messages to `sink`.
    pub fn new(volume: Arc<dyn Volume>, sink: Arc<dyn EventSink>) -> Self {
        Self {
            volume,
            sink,
            cancelled: Mutex::new(HashSet::new()),
        }
    }

    /// Whether [`DemoAbout::invoke`] answers `command`.
    pub fn handles(command: &str) -> bool {
        ABOUT_COMMANDS.contains(&command)
    }

    /// Runs `command` with Tauri-style `args` (camelCase keys). Every one answers
    /// `null`: the card's answers go out on its channel.
    pub async fn invoke(&self, command: &str, args: Value) -> Result<Value, Value> {
        match command {
            "about_start" => {
                let a: StartArgs = parse(command, args)?;
                self.start(a).await;
            }
            "about_cancel" => {
                let a: RequestArgs = parse(command, args)?;
                self.cancelled.lock_ignore_poison().insert(a.request_id);
            }
            // No verdict runs in the demo, so there's none to stop.
            "about_stop_verdict" => {
                let _: RequestArgs = parse(command, args)?;
            }
            _ => return Err(Value::String(format!("the browser demo doesn't answer `{command}`"))),
        }
        Ok(Value::Null)
    }

    async fn start(&self, a: StartArgs) {
        let id = a.request.request_id.clone();
        let card = Card {
            about: self,
            channel: &a.on_event,
            request_id: &id,
        };
        // A card closed before this start arrived gathers nothing: its sends all stop.
        self.gather(&card, &a.request).await;
        // The channel ends, as Tauri ends one the command drops.
        card.end();
        self.cancelled.lock_ignore_poison().remove(&id);
    }

    /// The card for `request`: `unavailable` alone, or `planned`, the verdict's
    /// `aiOff`, one `fact` per row, and `factsSettled`.
    async fn gather(&self, card: &Card<'_>, request: &AboutRequest) {
        let path = request.path.as_path();
        let entry = match self.volume.get_metadata(&AppPath::legacy_unchecked(path)).await {
            Ok(entry) => entry,
            Err(error) => {
                card.send(&AboutUpdate::Unavailable {
                    why: unavailable_why(&error),
                });
                return;
            }
        };
        let target = target_of(&entry);
        // The path's last name, as the desktop's plan reads it (`Path::extension`).
        let extension = (target == AboutTarget::File)
            .then(|| path.extension().map(|e| e.to_string_lossy().into_owned()))
            .flatten();
        let caps = VolumeCaps {
            // The bytes are in memory: reading the first and last of them is free.
            content: true,
            ..VolumeCaps::identity_only(ItemText::new(self.volume.name()))
        };
        let mut planned = plan(PlanInput {
            target,
            extension: extension.as_deref(),
            is_executable: false,
            contents_local: true,
            caps: &caps,
            os: &NO_OS_RECORDS,
            owner: &OwnerQuestion::Nothing,
            in_a_repo: false,
            git_snapshot: false,
            image_format: false,
        });
        planned.retain_kinds(|kind| ANSWERED.contains(&kind));
        let action_caps = ActionCaps {
            removal: self.volume.is_writable().then(|| {
                if self.volume.has_trash() {
                    RemoveBy::Trash
                } else {
                    RemoveBy::Delete
                }
            }),
            // No file manager to show it in, and no Ask Cmdr, from a page.
            ..ActionCaps::none()
        };
        let mut about = AboutCard::new(&planned);
        if !card.send(&AboutUpdate::Planned {
            derived: derive(&about, &action_caps),
            plan: planned.clone(),
        }) {
            return;
        }
        // The demo has no AI, as a desktop with AI off: the card draws no verdict block.
        if !card.send(&AboutUpdate::VerdictUnavailable {
            why: VerdictUnavailable::AiOff,
        }) {
            return;
        }
        for kind in planned.kinds() {
            let fact = match kind {
                FactKind::Identity => AboutFact::Identity(Fact::Known(identity_of(&entry))),
                FactKind::RealType => AboutFact::RealType(self.real_type_of(path, &entry, extension.as_deref()).await),
                // `retain_kinds` kept only the rows above.
                _ => AboutFact::unknown(kind, UnknownWhy::VolumeCantTell),
            };
            if about.apply(fact.clone()) == Applied::NotPlanned {
                continue;
            }
            if !card.send(&AboutUpdate::Fact {
                fact,
                derived: derive(&about, &action_caps),
            }) {
                return;
            }
        }
        card.send(&AboutUpdate::FactsSettled);
    }

    /// The real-type row: the file's first [`HEAD_BYTES`] and, past them, its last
    /// [`TAIL_BYTES`], sniffed against its extension, as the desktop's local job reads them.
    async fn real_type_of(&self, path: &Path, entry: &FileEntry, extension: Option<&str>) -> Fact<RealType> {
        let len = entry.size.unwrap_or(0);
        let head = match self
            .volume
            .read_range(&AppPath::legacy_unchecked(path), 0, HEAD_BYTES)
            .await
        {
            Ok(head) => head,
            Err(error) => return RealType::by_name(extension, fact_why(&error)),
        };
        let tail = if len > HEAD_BYTES as u64 {
            match self
                .volume
                .read_range(&AppPath::legacy_unchecked(path), len - TAIL_BYTES as u64, TAIL_BYTES)
                .await
            {
                Ok(tail) => Some(tail),
                Err(error) => return RealType::by_name(extension, fact_why(&error)),
            }
        } else {
            None
        };
        Fact::Known(real_type(&head, tail.as_deref(), extension))
    }

    fn is_cancelled(&self, request_id: &str) -> bool {
        self.cancelled.lock_ignore_poison().contains(request_id)
    }
}

/// One card's channel.
struct Card<'a> {
    about: &'a DemoAbout,
    channel: &'a str,
    request_id: &'a str,
}

impl Card<'_> {
    /// Sends `update`, unless the card was cancelled: then nothing more goes out,
    /// and the caller stops. Returns whether it went.
    fn send(&self, update: &AboutUpdate) -> bool {
        if self.about.is_cancelled(self.request_id) {
            return false;
        }
        let message = AboutEvent {
            request_id: self.request_id,
            update,
        };
        match serde_json::to_value(&message) {
            Ok(message) => {
                self.about.sink.emit(
                    CHANNEL_EVENT,
                    serde_json::json!({ "channel": self.channel, "message": message }),
                );
                true
            }
            Err(_) => {
                // No error text: a serializer's message can quote a value from the card.
                log::error!("an About card's message didn't serialize");
                false
            }
        }
    }

    /// The channel's end: nothing more follows on it.
    fn end(&self) {
        self.about.sink.emit(
            CHANNEL_EVENT,
            serde_json::json!({ "channel": self.channel, "end": true }),
        );
    }
}

/// The headline, removal row, and actions for the card as it stands.
fn derive(card: &AboutCard, caps: &ActionCaps) -> Derived {
    Derived {
        headline: headline(card),
        removal: consequences(card),
        actions: actions(card, caps),
    }
}

/// What the card is about, from the volume's own answer. The demo tree holds no
/// app bundles or packages, so a folder is a folder.
fn target_of(entry: &FileEntry) -> AboutTarget {
    if entry.is_symlink {
        AboutTarget::Symlink
    } else if entry.is_directory {
        AboutTarget::Folder
    } else {
        AboutTarget::File
    }
}

/// What the volume's metadata says, as the identity row (the desktop's
/// `through::identity_of`).
fn identity_of(entry: &FileEntry) -> Identity {
    let seconds = |at: Option<u64>| at.and_then(|at| i64::try_from(at).ok());
    let is_dir = entry.is_directory;
    Identity {
        name: ItemText::new(&entry.name),
        name_hid: hidden_in_name(&entry.name),
        logical_size: if is_dir { None } else { entry.size },
        physical_size: if is_dir { None } else { entry.physical_size },
        created: seconds(entry.created_at),
        modified: seconds(entry.modified_at),
        // The in-memory volume has one name per node.
        hard_links: None,
        symlink_target: None,
    }
}

/// Why the card can't be gathered at all. ❌ Never by the error's message.
fn unavailable_why(error: &VolumeError) -> AboutUnavailable {
    match error {
        VolumeError::NotFound(_) | VolumeError::NotADirectory(_) => AboutUnavailable::NotFound,
        VolumeError::PermissionDenied { .. } => AboutUnavailable::PermissionDenied,
        _ => AboutUnavailable::Unreadable,
    }
}

/// Why a row's read came back without an answer (the desktop's `through::why_of`).
fn fact_why(error: &VolumeError) -> UnknownWhy {
    match error {
        VolumeError::NotFound(_) | VolumeError::NotADirectory(_) => UnknownWhy::NotFound,
        VolumeError::PermissionDenied { .. } => UnknownWhy::PermissionDenied,
        VolumeError::NotSupported => UnknownWhy::VolumeCantTell,
        _ => UnknownWhy::Unparseable,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartArgs {
    request: AboutRequest,
    /// The card's `Channel`, as it crosses the wire (`__CHANNEL__:…`).
    on_event: String,
}

/// The desktop's `about::event::AboutRequest`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AboutRequest {
    request_id: String,
    /// As the pane holds it: the demo's volume is POSIX, so the wire text is the volume's path.
    path: PathBuf,
    // The demo has one volume, and plans from the volume's own answer, so neither the
    // pane's volume id nor its folder hint changes the card.
    #[serde(default)]
    #[allow(dead_code, reason = "part of the wire; the demo's one volume answers it")]
    volume_id: Option<String>,
    #[serde(default)]
    #[allow(dead_code, reason = "part of the wire; the volume's answer decides the target")]
    is_directory: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequestArgs {
    request_id: String,
}

// The wire, mirrored from the desktop's `about/event.rs` (the app crate can't build
// for the browser): the subset the demo sends. `about_test.rs` pins each against
// `bindings.ts`.

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AboutEvent<'a> {
    request_id: &'a str,
    update: &'a AboutUpdate,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
enum AboutUpdate {
    Planned { plan: AboutPlan, derived: Derived },
    Fact { fact: AboutFact, derived: Derived },
    FactsSettled,
    Unavailable { why: AboutUnavailable },
    VerdictUnavailable { why: VerdictUnavailable },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Derived {
    headline: Headline,
    removal: Removal,
    actions: Actions,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
enum AboutUnavailable {
    NotFound,
    PermissionDenied,
    Unreadable,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum VerdictUnavailable {
    AiOff,
}
