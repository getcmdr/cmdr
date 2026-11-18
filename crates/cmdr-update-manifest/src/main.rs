//! The release gate: refuses a `latest.json` any shipped Cmdr couldn't read.
//!
//! ```text
//! cargo run --locked -p cmdr-update-manifest --bin cmdr-check-update-manifest -- latest.json
//! ```
//!
//! Runs every reader in `history` (the Tauri plugin of Cmdr 0.7 and earlier, Cmdr 0.8 to 0.61,
//! and this build's) over the file. Exits 1 if any refuses it, 2 on bad usage. The release
//! pipeline's `publish` job runs it before the upload; `scripts/release-assets` validates the
//! release-side rules (`validate.go`).

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "a command-line gate: its verdicts are its output"
)]

use cmdr_update_manifest::{MAX_MANIFEST_BYTES, history};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [path] = args.as_slice() else {
        eprintln!("Usage: cmdr-check-update-manifest <latest.json>");
        std::process::exit(2);
    };
    let body = match std::fs::read(path) {
        Ok(body) => body,
        Err(e) => {
            eprintln!("Couldn't read {path}: {e}");
            std::process::exit(2);
        }
    };
    println!(
        "{path} ({} bytes, the app reads up to {MAX_MANIFEST_BYTES}):",
        body.len()
    );
    match history::gate(&body) {
        Ok(report) => {
            for line in report {
                println!("    {line}");
            }
        }
        Err(problems) => {
            for problem in &problems {
                println!("    REFUSED: {problem}");
            }
            eprintln!("Installs out there would fail to read this manifest. Nothing was published.");
            std::process::exit(1);
        }
    }
}
