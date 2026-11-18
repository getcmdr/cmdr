# S3 backend: data-safety hunt

Slice: `crates/cmdr-s3/`, `apps/desktop/src-tauri/src/s3_costs/`, and the desktop S3 glue. Five findings survive: one
high, two medium, two low.

### s3-1. Deleting or moving a markerless folder also deletes a same-named file beside it that was never selected

- **Severity:** high
- **Confidence:** confirmed for the shadowed-name trigger (traced end to end); plausible for the fresh-volume trigger
  (the frontend keeping a stale listing across a volume replacement isn't traced)
- **Where:** `crates/cmdr-s3/src/volume/mutation.rs:197`, `delete_impl`; `crates/cmdr-s3/src/volume/listing.rs:60`,
  `settle`; `crates/cmdr-s3/src/volume/paths.rs:81`, `resolve`; `crates/cmdr-s3/src/volume/paths.rs:115`,
  `note_beside_folders`; callers `crates/cmdr-operations/src/transfer/volume/source_sweep.rs:185` and the delete
  walker's per-directory `volume.delete`
- **Excerpt:**

```rust
// crates/cmdr-s3/src/volume/mutation.rs:197
        let target_key = match contents {
            FolderContents::Holds => {
                return Err(VolumeError::IoError {
                    message: format!("{remote} still holds something"),
                    raw_os_error: Some(ENOTEMPTY),
                });
            }
            FolderContents::MarkerOnly => prefix,
            FolderContents::Nothing if holder == Holder::Folder => return Err(VolumeError::NotFound(remote)),
            FolderContents::Nothing => {
                if self.head_object(&client, bucket, key, &remote).await?.is_none() {
                    return Err(VolumeError::NotFound(remote));
                }
                key.to_string()
            }
        };
        self.delete_key(&client, bucket, &target_key, &remote).await?;
```

```rust
// crates/cmdr-s3/src/volume/listing.rs:60
    let objects = objects.into_iter().filter_map(|object| match object {
        Child::Object {
            name,
            size,
            modified,
            archived,
            ..
        } if folders.contains(&name) => {
            let shown = format!("{name}{FILE_SUFFIX}");
            (!taken.contains(&shown)).then_some(Child::Object {
                name: shown,
                size,
                modified,
                archived,
                beside_folder: true,
            })
        }
        other => Some(other),
    });
```

- **Mechanism:** The only thing that stops a folder row's delete from reaching the file of its name is the in-memory
  `beside_folders` set, which `resolve` turns into `Holder::Folder`. When the set doesn't hold the path, the folder row
  resolves as `Holder::Either`. Then:
  1. The engine (the volume delete walker, or a move's `source_sweep`) deletes the folder's listed children with
     `delete_files`.
  2. It calls `delete(<folder>)`. `delete_impl` lists `<key>/`: with no `<key>/` marker and the children gone, that's
     `FolderContents::Nothing`.
  3. For `Holder::Either`, `Nothing` falls back to a HEAD of `<key>` and deletes the object there: the user's file
     `notes`, which they never selected (and on a move, never copied).

  Two ways the set misses the path:
  - **Shadowed name (confirmed):** when a real key `notes (file)` sits beside `notes` and `notes/…`, `settle` drops the
    `notes` file from the listing entirely (`filter_map` → `None`) and never marks it `beside_folder`, so
    `note_beside_folders` doesn't record `/<bucket>/<parent>/notes`. The folder row `notes` is then `Either` forever,
    and the hidden file goes with the folder. A move of the parent copies `notes (file)` and `notes/*`, but not the
    hidden `notes`, then deletes it in the sweep.
  - **A volume that never listed the parent (plausible):** `beside_folders` lives on `S3VolumeInner` and starts empty. A
    redial through `connect_and_register_since` builds a new `S3Volume` (`install_retiring_incumbent`), and an agent or
    MCP path operation can hit a volume that never listed the parent. A pane still showing the old listing (`notes` and
    `notes (file)`), or a path-based delete of `notes`, then takes the `Either` route above.

- **Trigger:** In the shadowed case, a bucket written by other tools (`aws s3 sync` writes no folder markers) holds
  `p/notes`, `p/notes/a.txt`, and `p/notes (file)`. The user deletes folder `notes`, or moves `p` elsewhere.
- **Guard ruled out:** `Holder::Folder` refuses the fallback (`mutation.rs:205`), but only when the set holds the path.
  `delete_files` correctly doesn't call `forget_beside_folder`, but that doesn't help when the entry was never there. A
  folder WITH a marker is safe (`MarkerOnly` deletes only the marker). `beside_folder_test.rs` covers the recorded case
  only. The engine passes no "this is a directory" to `Volume::delete`.
- **Impact:** permanent loss of an object the user didn't select. S3 has no trash; it's recoverable only from bucket
  versioning, if enabled.
- **Suggested fix:** Don't let a folder delete fall back to the object unless the caller asked about a file. For
  example, when `Either` finds `Nothing` but the HEAD finds an object, refuse (or answer `NotFound`) whenever `<key>/`
  ever had keys this operation saw. Or record a shadowed file in `beside_folders` too (so the folder row is `Folder`)
  and give `delete` the walker's kind.

### s3-2. A PUT cut off by a real network drop can leave a truncated object under the user's filename

- **Severity:** medium
- **Confidence:** plausible (the S3 side is traced; the engine's terminal reporting of the refused retry is read, not
  run)
- **Where:** `crates/cmdr-s3/src/volume/writes.rs:481`, `settle_cut_off_put`; `crates/cmdr-s3/src/volume/writes.rs:409`,
  `put_whole`; `crates/cmdr-s3/src/volume/volume_impl.rs:48`, `noting`;
  `crates/cmdr-operations/src/transfer/interrupted_landing.rs:133`
- **Excerpt:**

```rust
// crates/cmdr-s3/src/volume/writes.rs:476
        let token = target.metadata.write_token.as_deref()?;
        for attempt in 0..3 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
            let Ok(head) = self.head_object(client, target.bucket, target.key, target.remote).await else {
                return None;
            };
            let Some(head) = head else {
                continue;
            };
            if head.header(crate::metadata::WRITE_TOKEN_HEADER) != Some(token) {
                // The original, or another writer's: never ours to remove.
                return None;
            }
```

- **Mechanism:** The cut-off cleanup exists because some servers (VersityGW, reached as "Other") store a PUT's partial
  body under the final key. It runs only while the connection is still up:
  1. A one-PUT write goes to a FREE name (`CreateNew`; an overwrite on an untrusted provider goes as multipart, which is
     safe). The network drops mid-body. `client.upload` fails, and `put_whole` calls `settle_cut_off_put`
     (`writes.rs:410`).
  2. The HEAD fails on the dead link, and `let Ok(head) = … else { return None; }` gives up at once, with no later retry
     and no record of the token. If the silence watch fires first instead, `noting`'s `select!` drops the whole write
     future (`volume_impl.rs:51`) and no settle runs at all.
  3. Once the server notices the drop, it publishes the truncated body, carrying our token.
  4. The engine waits out the outage and runs the file again. `settle_unanswered_create` sees a size mismatch and
     returns `RunAgain`, which "leaves the name alone". The new attempt has a new token, and its `CreateNew`
     check-then-write HEAD finds the truncated object and returns `AlreadyExists`.
- **Trigger:** Copy or move files to an "Other" S3-compatible server that keeps cut-off bodies, and lose Wi-Fi or VPN
  mid-file.
- **Guard ruled out:** `refuses_short_body` doesn't help: the condition is that the provider is off it.
  `ShortStream::OnePart` covers overwrites only. The upload ledger covers multipart only, so nothing remembers a one-PUT
  token. The engine's `remove_unstaged_partial` only runs when `writing_at_the_final_name` is set by a `NotSupported`
  landing, which isn't the case here.
- **Impact:** A byte-incomplete object sits under the real filename at the destination, and the file is reported as
  "already exists". The user may Skip and keep a corrupt copy. The source survives (the move doesn't delete it), so this
  is corruption, not loss.
- **Suggested fix:** Persist the pending PUT's token (like the multipart ledger) and settle it after reconnect. Or have
  the retry recognize a short object carrying the previous attempt's token and remove it before its `CreateNew` check.

### s3-3. One unreadable entry in `s3_known_places.json` wipes every saved S3 place on the next connect

- **Severity:** medium
- **Confidence:** confirmed
- **Where:** `apps/desktop/src-tauri/src/network/server_list_file.rs:39`, `load`;
  `apps/desktop/src-tauri/src/network/s3_known_places.rs:293`, `load_known_s3_places`, and `:305`, `save`
- **Excerpt:**

```rust
// apps/desktop/src-tauri/src/network/server_list_file.rs:33
    let dir = crate::config::resolved_app_data_dir(app).ok()?;
    let path = dir.join(file_name);
    let tmp = path.with_extension("json.tmp");
    if tmp.exists() {
        let _ = fs::remove_file(&tmp);
    }
    let store = fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    Some((store, path))
```

- **Mechanism:**
  1. The file doesn't parse. For example, a downgrade after a newer build saved a provider `kind` this build's
     `S3ProviderChoice` (an internally tagged enum, with no catch-all variant) doesn't know, which fails the whole
     `Vec`.
  2. `load` silently answers an empty default and STILL hands back the path, so `STORE_PATH` is set.
  3. The next `s3_known_places::remember` (any successful connect, or a save from the edit sheet) calls `save()`, which
     durably writes the empty-plus-one store over the old file.
- **Trigger:** Install an older Cmdr over a newer one that knew another preset, or hand-edit the file with a typo, then
  connect to any S3 place.
- **Guard ruled out:** `durable_write_json` protects against a torn write, not this overwrite. There's no backup, no
  per-entry tolerance, and no refusal to save over a file that failed to parse. The same `load` serves
  `sftp_known_servers.rs` and `webdav_known_servers.rs`, so it's likely shared with the SFTP/WebDAV slice.
- **Impact:** All saved S3 accounts, bucket places, pins, account names, and auto-reconnect switches are lost. The
  secrets stay in the Keychain, so the user can re-add everything, but not without effort. Recoverable loss.
- **Suggested fix:** On a parse failure, move the file aside (for example, `.corrupt-<date>`) and don't set the save
  path from an unparsed file. Or deserialize places one by one and keep the ones that read.

### s3-4. A read stream never checks that it delivered the length it announced

- **Severity:** low
- **Confidence:** plausible (needs a server or proxy that answers a GET without `Content-Length` and closes the
  connection short, which hyper can't flag)
- **Where:** `crates/cmdr-s3/src/volume/streams.rs:58`, `judge_get`; `crates/cmdr-s3/src/volume/streams.rs:93`,
  `S3ReadStream::next_chunk`; consumer `crates/cmdr-operations/src/transfer/volume/stream_pipe.rs:439`
- **Excerpt:**

```rust
// crates/cmdr-s3/src/volume/streams.rs:50
    match status {
        StatusCode::PARTIAL_CONTENT => Answered::Window {
            total: range_total.unwrap_or_else(|| content_length.unwrap_or(0) + offset),
            skip: 0,
        },
        StatusCode::RANGE_NOT_SATISFIABLE => Answered::PastTheEnd {
            total: range_total.unwrap_or(0),
        },
        status if status.is_success() => Answered::Window {
            total: content_length.unwrap_or(0),
            skip: offset,
        },
        _ => Answered::Refused,
    }
```

- **Mechanism:** `next_chunk` returns `None` at the body's end without comparing `read` with `total`, and a missing
  `Content-Length` becomes `StreamLength::Known(0)`. The local writer writes whatever arrives, and `stream_pipe_file`
  commits `written.bytes` with no size check, so a body that ends early but cleanly lands as a short file. A move then
  sweeps the S3 source.
- **Trigger:** A move off an "Other" endpoint behind a proxy that strips `Content-Length` and drops mid-body.
- **Guard ruled out:** hyper errors on an early close only when `Content-Length` or chunked framing is present. All
  seven presets and both fixtures send `Content-Length`.
- **Impact:** A truncated download, and on a move, loss of the original. Low because the trigger needs a misbehaving
  intermediary.
- **Suggested fix:** At EOF, fail with an `IoError` when the stream announced a length and `read + offset` falls short
  of it. Treat a 2xx without any length as `StreamLength::Unknown`, never `Known(0)`.

### s3-5. Unfinished uploads recorded before an endpoint move are never swept

- **Severity:** low
- **Confidence:** confirmed
- **Where:** `crates/cmdr-s3/src/volume/multipart_upload.rs:600`, `S3VolumeInner::account`;
  `crates/cmdr-s3/src/volume/upload_ledger.rs:126`, `leftovers`; `apps/desktop/src-tauri/src/server_move.rs:282`,
  `edit_s3_account`
- **Excerpt:**

```rust
// crates/cmdr-s3/src/volume/multipart_upload.rs:600
    pub(super) fn account(&self) -> String {
        let params = self.params();
        format!("{}#{}", params.credential_service(), params.access_key_id())
    }
```

```rust
// crates/cmdr-s3/src/volume/upload_ledger.rs:126
        open.retain(|record| record.account == account && !registry.in_flight.contains(record));
```

- **Mechanism:** A record's account is `s3+<scheme>://<host>:<port>#<key id>`. `edit_s3_account` relocates an "Other"
  account to a new endpoint, so every later sweep asks for the new account string. The old records match nothing, so
  `compact` keeps them forever and nothing ever aborts them. The same happens to a place the user forgets.
- **Trigger:** An upload is cut off (crash, quit deadline), then the user edits the server's address before the next
  connect sweeps it.
- **Guard ruled out:** The sweep runs only for the connecting account. Nothing migrates records in `server_move.rs`.
- **Impact:** Cmdr's own state, not user files: the orphaned parts stay billed on the server until a lifecycle rule
  removes them.
- **Suggested fix:** Have `edit_s3_account` rewrite the ledger's account field, or key records by something that
  survives a move (the account's root place id before the move, mapped).

## Covered

Read in full:

- `crates/cmdr-s3/CLAUDE.md`, `crates/cmdr-s3/DETAILS.md`
- `crates/cmdr-s3/src/volume/mutation.rs`, `paths.rs`, `listing.rs`, `query.rs`, `batch.rs`, `writes.rs`,
  `multipart_upload.rs`, `upload_ledger.rs`, `upload.rs`, `server_copy.rs`, `streams.rs`, `volume_impl.rs`,
  `share_link.rs`, `errors.rs`, `state.rs`
- `crates/cmdr-s3/src/encoding.rs`, `crates/cmdr-s3/src/xml/build.rs`
- `apps/desktop/src-tauri/src/commands/s3.rs`, `commands/servers/s3_accounts.rs`, `commands/share_link.rs`,
  `network/s3_volume_wiring.rs`, `s3_costs/price_source.rs`

Read in part:

- `crates/cmdr-s3/src/volume/reconnect.rs` (first 110 lines), `volume/mod.rs` (connect and assemble), `transport/mod.rs`
  (routing, `note_sent`), `transport/answer.rs` (`object_length`, `Opened`), `ops.rs` (the list, copy, delete, and
  share-link builders), `sigv4.rs` (`Credentials` Debug, `PayloadHash`), `request.rs`, `error.rs` (predicates)
- `apps/desktop/src-tauri/src/network/s3_known_places.rs` (types, load, save), `server_move.rs` (`edit_s3_account`,
  `follow`), `network/server_list_file.rs`, `s3_costs/mod.rs`
- Engine context: `crates/cmdr-operations/src/transfer/volume/stream_pipe.rs`, `source_sweep.rs`,
  `transfer/interrupted_landing.rs`, `staged_write.rs::failed_write_leaves_ours_at`, `delete/walker.rs` (batch loop),
  and `crates/cmdr-local/src/local_posix/streams.rs::write_from_stream_impl`

## Not covered

- `crates/cmdr-s3/src/volume/upload_body.rs` (the pause hold and body pieces), `scan.rs`, `routing.rs` in full,
  `profile.rs` in full (the allowlists were taken from DETAILS), `refusal.rs`, `params.rs`, `multipart.rs`
  (`plan_parts`), `metadata.rs`, `xml/parse.rs`, `cost/*`, the rest of `transport/mod.rs` (`upload`, `open`, `send`),
  and `sigv4.rs` signing.
- `apps/desktop/src-tauri/src/s3_costs/plan.rs`, the rest of `s3_known_places.rs` (`relocate_account`, `forget`, and
  migrate), `favorites/reach.rs`, `listing_index_sizes` S3 bits, `file_viewer` materialize for S3, and the MCP volume
  resource.
- Whether the frontend relists a pane when an S3 volume is superseded under the same id (s3-1's second trigger).

## Near misses

- **A folder delete prefix-matching `foo-bar/`:** every "what's under it?" listing asks `<key>/` with the trailing slash
  (`mutation.rs:180`, `query.rs:186`, `batch.rs:66`), and `delete_files` deletes by exact key, never by prefix.
- **Hidden keys (`a//b`, `a/.`, `a/..`) lost by a folder move or delete:** `children_of` hides them, but
  `folder_contents` still counts their prefixes and objects, so the folder's delete answers `ENOTEMPTY` and they stay.
- **`..` or `/` in a listed name escaping a download target:** `addressable` drops empty, `.`, and `..` names, names
  never hold `/`, and the engine joins through `ChildName`. A hostile `ListBuckets` name isn't validated here, but
  `ChildName` covers it.
- **Dot segments reaching another object:** `encode_key` refuses `.` and `..` (`KeyError::DotSegment`), and
  `DeleteObjects` keys travel in the body, XML-escaped with CR and LF as references.
- **Copy then delete losing the source on a failed copy:** `rename_impl` deletes only after `judge_landing` returns
  `Verified`. A multipart copy aborts on `SourceChanged`, and the engine never sweeps an unfinished copy's source.
- **A rename onto itself deleting the object:** the `remote_from == remote_to` check runs on resolved paths, so
  `notes (file)` → `notes` (same key) is a no-op. R2's NFD/NFC twins could defeat a byte comparison, but listings come
  back NFC and new names are composed (`composes_new_names`), so no UI path produces the mismatch.
- **An overwrite off the `refuses_short_body` allowlist publishing a truncated body over the original:** it goes as a
  one-part multipart upload (`overwrites_in_parts`).
- **A late cancel deleting a published overwrite:** after the last piece is released, the PUT waits for its answer
  (`writes.rs:367`), and `settle_cut_off_put` never deletes ours at full size.
- **The sweep aborting a live upload:** it skips `in_flight` (process-wide) and aborts only recorded ids. A second
  process on the same data dir is a documented, accepted risk.
- **A completion abort racing a completion that did land:** `landed_whole` checks the token at full size before
  aborting, and a lost completion only ever reports failure, so a move keeps its source.
- **`ReplacedAfterUs` letting a move delete its source while another writer's object holds the name:** a documented
  "newer write wins" decision, the same as on any filesystem.
- **`settle_cut_off_put` deleting our whole object when HEAD omits `Content-Length`:** every provider sends it, or GCS's
  `x-goog-stored-content-length` fallback applies.
- **Write and copy paths ignoring `Holder::Folder`:** `to_remote_path` drops the holder in `write_from_stream`,
  `create_file`, `copy_on_server`, and `delete_files`. A folder-row path reaching them needs the engine to treat a
  listed folder as a file, and no trace found one (a file-vs-folder clash is a cross-type conflict).
- **Credentials in logs:** `Credentials`' `Debug` redacts the secret, API calls sign by header (no signature in a URL),
  `ShareLink` never logs or crosses IPC, and the secret lives in the Keychain, not `s3_known_places.json`.
- **The price cache:** a temp-then-rename write of non-user data, refused when it doesn't parse.
