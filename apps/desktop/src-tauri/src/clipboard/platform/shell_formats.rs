//! The Windows Shell's clipboard formats for files, as bytes: `CF_HDROP`'s `DROPFILES` block,
//! the `DROPEFFECT` DWORD the registered `Preferred DropEffect` and `Paste Succeeded` formats
//! carry, and the paths as `CF_UNICODETEXT`. Pure, so every OS's tests pin the layout; only
//! the Windows arm (`platform/windows.rs`) puts them on a clipboard.

use cmdr_fs::app_path::AppPath;

/// `DROPEFFECT_NONE`: the target did nothing the source has to finish (an optimized move's
/// `Performed DropEffect`).
#[cfg_attr(test, allow(dead_code, reason = "used by the Windows arm only"))]
pub(crate) const DROPEFFECT_NONE: u32 = 0;
/// `DROPEFFECT_COPY`: the files go on paste as a copy.
pub(crate) const DROPEFFECT_COPY: u32 = 1;
/// `DROPEFFECT_MOVE`: the files were cut, so a paste moves them.
pub(crate) const DROPEFFECT_MOVE: u32 = 2;

/// `sizeof(DROPFILES)`: `pFiles` (u32), `pt` (two i32), `fNC` (BOOL), `fWide` (BOOL).
const DROPFILES_HEADER: usize = 20;

/// Whether a `CF_HDROP` block holds its whole header and its list starts inside it: another
/// app's malformed block would send `DragQueryFileW` reading past the end.
pub(crate) fn dropfiles_header_fits(block: &[u8]) -> bool {
    let Some(p_files) = block.first_chunk::<4>() else {
        return false;
    };
    let p_files = u32::from_le_bytes(*p_files) as usize;
    block.len() >= DROPFILES_HEADER && p_files >= DROPFILES_HEADER && p_files < block.len()
}

/// The `CF_HDROP` block for `paths` (each a path's UTF-16 units, no NUL): the `DROPFILES`
/// header with `pFiles` pointing just past it and `fWide` set, then each path NUL-terminated,
/// then the list's closing NUL. Little-endian, as Windows reads it.
pub(crate) fn dropfiles_block(paths: &[Vec<u16>]) -> Vec<u8> {
    let units: usize = paths.iter().map(|p| p.len() + 1).sum::<usize>() + 1;
    let mut out = Vec::with_capacity(DROPFILES_HEADER + units * 2);
    out.extend_from_slice(&(DROPFILES_HEADER as u32).to_le_bytes()); // pFiles
    out.extend_from_slice(&0i32.to_le_bytes()); // pt.x
    out.extend_from_slice(&0i32.to_le_bytes()); // pt.y
    out.extend_from_slice(&0i32.to_le_bytes()); // fNC
    out.extend_from_slice(&1i32.to_le_bytes()); // fWide
    for path in paths {
        push_units(&mut out, path);
        out.extend_from_slice(&[0, 0]);
    }
    out.extend_from_slice(&[0, 0]);
    out
}

/// Whether a native path another app put in `CF_HDROP`, already crossed to `app`, names that
/// file for certain: a verbatim `\\?\` path (`units`) names exactly its file; a PLAIN one only
/// when its plain form is the same file (`C:\x\a.` is `C:\x\a` to Win32, `C:\x\CON` the device,
/// past 259 units an API may truncate). Pure, so every OS's tests pin it; the crossing itself
/// (`from_native_unicode`, which refuses a device, `C:foo`, a relative path) is the caller's.
pub(crate) fn names_its_file(units: &[u16], app: &AppPath) -> bool {
    let verbatim: Vec<u16> = r"\\?\".encode_utf16().collect();
    units.starts_with(&verbatim) || app.to_native_windows_plain().is_ok()
}

/// A `DROPEFFECT` value as the four bytes its clipboard format holds.
pub(crate) fn effect_bytes(effect: u32) -> [u8; 4] {
    effect.to_le_bytes()
}

/// Whether a `Preferred DropEffect` value marks the files as CUT: the move bit without the copy
/// bit. Explorer's Ctrl+X writes `DROPEFFECT_MOVE` alone; its Ctrl+C writes copy and link.
/// Fewer than four bytes says nothing, so it reads as a copy.
pub(crate) fn effect_is_cut(bytes: &[u8]) -> bool {
    let Some(word) = bytes.first_chunk::<4>() else {
        return false;
    };
    let effect = u32::from_le_bytes(*word);
    effect & DROPEFFECT_MOVE != 0 && effect & DROPEFFECT_COPY == 0
}

/// The paths as `CF_UNICODETEXT` bytes: one per line (CRLF, as Windows text runs), NUL-terminated,
/// so pasting into a text editor gives the paths, as Finder's copy does on macOS.
pub(crate) fn paths_text(paths: &[Vec<u16>]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, path) in paths.iter().enumerate() {
        if i > 0 {
            push_units(&mut out, &[u16::from(b'\r'), u16::from(b'\n')]);
        }
        push_units(&mut out, path);
    }
    out.extend_from_slice(&[0, 0]);
    out
}

fn push_units(out: &mut Vec<u8>, units: &[u16]) {
    for unit in units {
        out.extend_from_slice(&unit.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn the_dropfiles_block_is_a_wide_header_then_a_double_nul_terminated_list() {
        let block = dropfiles_block(&[units(r"C:\a"), units(r"\\nas\share\b")]);
        assert_eq!(&block[..4], &20u32.to_le_bytes(), "pFiles points past the header");
        assert_eq!(&block[16..20], &1i32.to_le_bytes(), "fWide");
        let list: Vec<u16> = block[20..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let mut expected = units(r"C:\a");
        expected.push(0);
        expected.extend(units(r"\\nas\share\b"));
        expected.extend([0, 0]);
        assert_eq!(list, expected);
    }

    #[test]
    fn a_name_outside_the_bmp_crosses_as_its_surrogate_pair() {
        let block = dropfiles_block(&[units("C:\\🦀")]);
        // `C`, `:`, `\`, the crab's two surrogates, the path's NUL, the list's NUL.
        assert_eq!(block.len(), 20 + (3 + 2 + 1 + 1) * 2);
        assert_eq!(
            &block[26..30],
            &[0x3E, 0xD8, 0x80, 0xDD],
            "U+1F980 as D83E DD80, little-endian"
        );
    }

    /// Whether the native `plain` (as another app wrote it), crossed to `app`, names its file.
    fn names(native: &str, app: &str) -> bool {
        let app =
            cmdr_fs::app_path::AppPathBuf::parse(app, cmdr_fs::app_path::PathFlavor::Windows).expect("an app path");
        names_its_file(&units(native), &app)
    }

    #[test]
    fn a_plain_path_naming_its_file_passes() {
        assert!(names(r"C:\Users\test_user_1\a.txt", "C:/Users/test_user_1/a.txt"));
        assert!(names(r"\\testnas.local\share\b.txt", "//testnas.local/share/b.txt"));
    }

    /// Win32 reads these plain paths as another file or none, so the paste mustn't guess.
    #[test]
    fn a_plain_path_win32_reads_as_another_file_is_refused() {
        assert!(!names(r"C:\x\a.", "C:/x/a."), "trailing dot: Win32 opens `a`");
        assert!(!names(r"C:\x\a ", "C:/x/a "), "trailing space");
        assert!(!names(r"C:\x\CON", "C:/x/CON"), "the console device");
        assert!(!names(r"C:\x\nul.txt", "C:/x/nul.txt"), "the null device");
        let long = "a".repeat(300);
        assert!(!names(&format!(r"C:\{long}"), &format!("C:/{long}")), "past 259 units");
    }

    /// A verbatim path names exactly its file, trailing dot and all.
    #[test]
    fn a_verbatim_path_passes_as_it_is() {
        assert!(names(r"\\?\C:\x\a.", "C:/x/a."));
    }

    #[test]
    fn a_block_whose_list_starts_past_its_end_doesnt_fit() {
        let block = dropfiles_block(&[units(r"C:\a")]);
        assert!(dropfiles_header_fits(&block));
        assert!(!dropfiles_header_fits(&block[..12]), "shorter than its header");
        let mut past = block.clone();
        past[..4].copy_from_slice(&(block.len() as u32).to_le_bytes());
        assert!(!dropfiles_header_fits(&past), "pFiles at the end");
        let mut inside = block;
        inside[..4].copy_from_slice(&4u32.to_le_bytes());
        assert!(!dropfiles_header_fits(&inside), "pFiles inside the header");
    }

    #[test]
    fn explorer_cut_reads_as_cut_and_its_copy_does_not() {
        assert!(effect_is_cut(&effect_bytes(DROPEFFECT_MOVE)));
        assert!(!effect_is_cut(&effect_bytes(DROPEFFECT_COPY)));
        // Explorer's Ctrl+C: copy and link.
        assert!(!effect_is_cut(&effect_bytes(DROPEFFECT_COPY | 4)));
        assert!(!effect_is_cut(&effect_bytes(DROPEFFECT_COPY | DROPEFFECT_MOVE)));
    }

    #[test]
    fn a_short_effect_reads_as_a_copy() {
        assert!(!effect_is_cut(&[2, 0]));
        assert!(!effect_is_cut(&[]));
    }

    #[test]
    fn the_text_is_one_path_per_line_and_nul_terminated() {
        let text = paths_text(&[units(r"C:\a"), units(r"C:\b")]);
        let decoded: Vec<u16> = text
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let mut expected = units("C:\\a\r\nC:\\b");
        expected.push(0);
        assert_eq!(decoded, expected);
    }
}
