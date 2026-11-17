# Which names exFAT, FAT32, and an SMB share take, read on a Mac (2026-11-18)

The evidence behind `NameRules` (`crates/cmdr-fs/src/validation.rs`, `crates/cmdr-fs/DETAILS.md` § "Name rules"),
gathered for task 14 of the Windows port (`docs/specs/windows-port-plan.md`, issue #942).

## Setup

- macOS 27.0 (Darwin 27.0.0), Apple Silicon. Both FAT images are FSKit-mounted (`mount` shows `fskit`).
- exFAT and FAT32: one 64 MB `hdiutil create -fs ExFAT` / `-fs "MS-DOS FAT32"` image each, `-layout MBRSPUD`, attached
  once with `-nobrowse` and detached once with `hdiutil detach` (the FSKit-safe discipline in
  `crates/cmdr-index/src/indexing/tests/CLAUDE.md`).
- SMB: a QNAP NAS (QTS, Samba with `vfs objects = … catia fruit … streams_depot`, `mangled names = yes`, a ZFS pool
  behind it), reached two ways: macOS smbfs (`/Volumes/<share>`), and `smb2-cli` (the `smb2` crate Cmdr's `SmbVolume`
  speaks). Server-side bytes read over SSH with `hexdump`.
- The probe: a small Go program creating each name with `O_CREAT|O_EXCL`, then listing the folder. Raw FAT bytes read by
  scanning the detached images for UTF-16LE spellings. APFS (the boot disk) as the control.

## What happened to each name

- **APFS**: takes everything but `/` and NUL, including `CON`, `a:b`, `trail.`, and control characters.
- **exFAT and FAT32 through macOS's drivers**: every create SUCCEEDED, and every name lists back as typed. On disk the
  driver writes the characters the format can't hold as Services for Macintosh private-use stand-ins: `"` U+F020, `*`
  U+F021, `:` U+F022, `<` U+F023, `>` U+F024, `?` U+F025, `\` U+F026, `|` U+F027, a control character 0x01–0x1F as
  U+F001–U+F01F, a trailing space U+F028, a trailing period U+F029 (`trail..` is `trail.` + U+F029). DEL (0x7F) and a
  leading space stay as themselves. `CON`, `NUL`, `con.txt` are stored as plain names. So nothing fails on the Mac, but
  Windows (which doesn't map the private-use range back) shows those files under other names, and can't open a reserved
  name at all.
- **QNAP share, smbfs and smb2 alike**: every create SUCCEEDED. Both clients carry the forbidden characters and trailing
  space or period in the same private-use range, and the server stores those bytes (`a:b` is `61 ef 80 a2 62` on the ZFS
  disk), so they round-trip. But a Windows device name (`CON`, `NUL`, `AUX`, `PRN`, `COM1`, `con.txt`, `nul.tar.gz`) is
  stored as itself and then LISTED to every client under a mangled 8.3 name: `CON` reads `CR23RR~9`, `con.txt` reads
  `CSHOFG~F.TXT`. The file the person named never shows under that name. `LPT9`, `CONIN$`, `COM¹`, and `CON .txt` list
  as themselves here (Samba's own reserved list is shorter than Windows'; Cmdr refuses Windows' full list, since Windows
  clients read the share too).
- **Name length**: APFS, exFAT, and FAT32 refuse past 255 UTF-16 units (`é` × 200, 400 bytes, is fine; `😀` × 128
  isn't). The share refuses past 255 BYTES (`é` × 128, 256 bytes, refused), its ZFS disk's rule. Every one takes a
  255-byte ASCII name, which Cmdr refused until this change (`>= 255`). 255 bytes stays the floor for every destination.
- **Case**: all four refuse `case.txt` beside `Case.txt` (`EEXIST`).

## What Cmdr does with it

- exFAT, FAT32, NTFS → `NameRules::Windows`: refuse the stand-in characters, control characters, a trailing space or
  period, and the reserved names, because what would land isn't the name typed. A product call, flagged for David: macOS
  itself allows these on a stick.
- SMB → `NameRules::SmbShare`: refuse only the reserved device names, the one class that visibly breaks.
- Everything else → `NameRules::Posix`.
