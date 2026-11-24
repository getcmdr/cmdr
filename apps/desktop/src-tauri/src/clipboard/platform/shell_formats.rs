//! The Windows Shell's clipboard formats for files, as bytes: `CF_HDROP`'s `DROPFILES` block,
//! the `DROPEFFECT` DWORD the registered `Preferred DropEffect` and `Paste Succeeded` formats
//! carry, and the paths as `CF_UNICODETEXT`. Pure, so every OS's tests pin the layout; only
//! the Windows arm (`platform/windows.rs`) puts them on a clipboard.

/// `DROPEFFECT_COPY`: the files go on paste as a copy.
pub(crate) const DROPEFFECT_COPY: u32 = 1;
/// `DROPEFFECT_MOVE`: the files were cut, so a paste moves them.
pub(crate) const DROPEFFECT_MOVE: u32 = 2;

/// `sizeof(DROPFILES)`: `pFiles` (u32), `pt` (two i32), `fNC` (BOOL), `fWide` (BOOL).
const DROPFILES_HEADER: usize = 20;

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
