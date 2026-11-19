//! "Calculate folder sizes" (Alt+Shift+Enter, Space on a folder) over the demo's
//! panes, answered as the desktop's `count_folder_sizes` answers a volume its index
//! covers (`apps/desktop/src-tauri/src/listing_index_sizes/count/mod.rs::plan`).
//!
//! The fold (`folder_sizes.rs`) stands in for an enabled index and gives every
//! folder row its exact size as the row is read, so a plan picks only a row the fold
//! hasn't sized, which a listing never holds. Such a row would get the fold's size
//! through the listing's own `refresh_index_sizes`, which is the count here: there's
//! no walk to run, so nothing is ever left to cancel.

use cmdr_listing::{ListingId, Listings};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::invoke::{answer, ok, parse};

/// Every command [`count`] answers, by its Tauri name.
pub const FOLDER_COUNT_COMMANDS: &[&str] = &["count_folder_sizes", "cancel_folder_size_count"];

/// Runs one of [`FOLDER_COUNT_COMMANDS`] over `listings`.
pub fn invoke(listings: &Listings, command: &str, args: Value) -> Result<Value, Value> {
    match command {
        "count_folder_sizes" => {
            let a: CountArgs = parse(command, args)?;
            answer(count(listings, &a.listing_id, a.include_hidden, a.paths.as_deref()))
        }
        // No count runs past its own call, so none is ever running to stop.
        "cancel_folder_size_count" => ok(false),
        other => Err(Value::String(format!("the browser demo doesn't answer `{other}`"))),
    }
}

/// The desktop's plan over an index-covered volume: the folder rows (no links, just
/// `only` when given) that have no size yet. Each gets the fold's size, as a walk's
/// reading would land.
fn count(
    listings: &Listings,
    listing_id: &str,
    include_hidden: bool,
    only: Option<&[String]>,
) -> Result<FolderSizeCountOutcome, CountFolderSizesError> {
    let id = ListingId::new(listing_id);
    let gone = || CountFolderSizesError::Gone {
        listing_id: listing_id.to_string(),
    };
    if listings.location(&id).is_none() {
        return Err(gone());
    }
    let planned = listings
        .with_rows(&id, include_hidden, |rows| {
            rows.iter()
                .filter(|entry| entry.is_directory && !entry.is_symlink)
                .filter(|entry| only.is_none_or(|paths| paths.contains(&entry.path)))
                .filter(|entry| entry.recursive_size.is_none())
                .count()
        })
        .map_err(|_| gone())?;
    // A count is the pane using its listing.
    listings.keep_alive(std::slice::from_ref(&id));
    if planned > 0 {
        listings.refresh_index_sizes(&id);
    }
    Ok(FolderSizeCountOutcome {
        counted: planned,
        unreadable: 0,
        cancelled: false,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CountArgs {
    listing_id: String,
    include_hidden: bool,
    paths: Option<Vec<String>>,
}

/// `FolderSizeCountOutcome` in the app (`listing_index_sizes/count/mod.rs`).
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct FolderSizeCountOutcome {
    counted: usize,
    unreadable: usize,
    cancelled: bool,
}

/// `CountFolderSizesError` in the app. `notConnected` can't happen: the one volume never goes.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
enum CountFolderSizesError {
    Gone { listing_id: String },
}
