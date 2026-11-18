//! `latest.json`: the reader lives in `cmdr-update-manifest`, shared with the release gate that
//! runs it (and every reader that ever shipped) over a manifest before it's published.
//! `crates/cmdr-update-manifest/CLAUDE.md`.

pub use cmdr_update_manifest::{MAX_MANIFEST_BYTES, PlatformEntry, UpdateInfo, UpdateManifest, check_manifest, parse};

#[cfg(test)]
mod tests {
    use cmdr_update_manifest::history::{Reader, read};
    use serde_json::json;

    const WRITER_BUILT: &str = include_str!("../../../../../scripts/release-assets/testdata/v0.61.0/latest.built.json");

    /// A named change to a manifest.
    type Edit = (&'static str, fn(&mut serde_json::Value));

    /// Cmdr 0.7 and earlier read `latest.json` through `tauri-plugin-updater` itself, and the
    /// release gate runs a copy of that reader (`history::Reader::TauriPlugin`), since the gate
    /// can't build the plugin. This holds the copy to the real type: on every manifest below,
    /// both accept or both refuse.
    #[test]
    fn the_plugin_reads_what_its_mirror_reads() {
        let built: serde_json::Value = serde_json::from_str(WRITER_BUILT).expect("JSON");
        let edits: [Edit; 12] = [
            ("as built", |_| {}),
            ("an added member", |b| b["channel"] = json!({ "beta": true })),
            ("a v-prefixed version", |b| b["version"] = json!("v0.61.0")),
            ("a non-semver version", |b| b["version"] = json!("latest")),
            ("a numeric version", |b| b["version"] = json!(61)),
            ("a date-only pub_date", |b| b["pub_date"] = json!("2026-11-13")),
            ("an offset pub_date", |b| {
                b["pub_date"] = json!("2026-11-13T15:59:56+01:00")
            }),
            ("numeric notes", |b| b["notes"] = json!(1)),
            ("null notes", |b| b["notes"] = json!(null)),
            ("a relative url", |b| {
                b["platforms"]["darwin-universal"]["url"] = json!("Cmdr.app.tar.gz")
            }),
            ("an entry without a signature", |b| {
                drop(
                    b["platforms"]["darwin-universal"]
                        .as_object_mut()
                        .map(|e| e.remove("signature")),
                );
            }),
            ("no platforms", |b| {
                drop(b.as_object_mut().map(|o| o.remove("platforms")))
            }),
        ];
        for (name, edit) in edits {
            let mut body = built.clone();
            edit(&mut body);
            let text = body.to_string();
            let plugin = serde_json::from_str::<tauri_plugin_updater::RemoteRelease>(&text);
            let mirror = read(Reader::TauriPlugin, text.as_bytes());
            assert_eq!(
                plugin.is_ok(),
                mirror.is_ok(),
                "{name}: the plugin says {:?}, its mirror {:?}",
                plugin.err().map(|e| e.to_string()),
                mirror.err()
            );
        }
    }
}
