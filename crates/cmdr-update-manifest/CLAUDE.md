# cmdr-update-manifest

`latest.json`, the update manifest every installed Cmdr polls: the app's reader, and every reader that ever shipped,
which the release gate (`cmdr-check-update-manifest`) runs over a candidate before `publish` uploads it. One home, so
the app and the gate can't disagree. App-free (`index-crate-isolation`).

## Module map

- `lib.rs`: `parse` (bounded, typed `ManifestRefusal`), `check_manifest`, the wire shape (`Wire`, `PlatformEntry`).
- `history.rs`: the shipped readers (`Reader::TauriPlugin` for Cmdr 0.7 and earlier, `Cmdr0_8` for 0.8.0–0.61.x,
  `Current`) and `gate`. `history/*.json`: published manifests, one per shape.
- `main.rs`: the `cmdr-check-update-manifest` binary. `tests.rs`: the contract, the history, proptest mutations.

## Must-knows

- **Fields may be added, ❌ never removed or retyped.** Old installs read the new file: one bad entry for ANY platform
  fails every Cmdr up to 0.61 and the 0.7 plugin. `a_removed_or_retyped_field_breaks_a_shipped_reader` pins it.
- **Lenient where optional, strict where required.** Another platform's entry or the advisory entry that doesn't read is
  dropped with a warn; a non-semver version or this install's own broken entry is a `ManifestRefusal`, which the app
  reports as an unreadable answer, ❌ never "up to date". The gate refuses anything this build would drop.
- **`TauriPlugin` mirrors `tauri-plugin-updater`'s `RemoteRelease`**, which the gate can't build;
  `updater/manifest.rs`'s `the_plugin_reads_what_its_mirror_reads` holds the copy to the real type. Bump the plugin?
  That test says whether the mirror still matches.
- **The writer shares the fixtures**: `scripts/release-assets/testdata/v0.61.0/latest{,.built}.json`, registered as
  `rustEmbeddedInputs`. A new published shape gets a `history/` file too.
- **`MAX_MANIFEST_BYTES` is 1 MiB**; the writer refuses over 256 KiB (`validate.go`), so the cap only meets
  non-manifests.

The reader's contract, the history and how it was recovered, the gate, and the surface: `DETAILS.md`. Read it before any
non-trivial work here: editing, planning, reorganizing, or advising.
