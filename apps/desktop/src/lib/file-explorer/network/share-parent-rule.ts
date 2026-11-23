/**
 * The rule behind "Show “..” in the top folder" (#1238): one default (the
 * `network.showShareParentRow` setting) plus a share's own override, which is stored
 * only while it differs from the default. Rust stores the same way
 * (`src-tauri/src/network/smb_share_parent.rs`); this is the frontend's read of it.
 */

/** What a share shows: its own choice where it has one, else the default. */
export function effectiveShow(override: boolean | null, defaultShow: boolean): boolean {
  return override ?? defaultShow
}

/**
 * The override a share holds after someone sets it to `show`: none when that's the
 * default, so flipping a share back to match the default clears its override.
 */
export function overrideAfter(show: boolean, defaultShow: boolean): boolean | null {
  return show === defaultShow ? null : show
}
