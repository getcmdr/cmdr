# Which names exFAT, FAT32, and an SMB share take, read on a Mac and on Linux (2026-11-18)

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
  255-byte ASCII name. So each filesystem's limit is counted in its own unit (`NameRules::name_limit`), and a share
  counts bytes, since a NAS's disk under Samba does.
- **Case**: all four refuse `case.txt` beside `Case.txt` (`EEXIST`).

## The same stick, read and written by Linux

Kernel 7.2.12 (OrbStack's Linux VM), an Alpine 3.20 container run `--privileged`, the two images loop-mounted with
`-t exfat` / `-t vfat`, `iocharset=utf8` (2026-11-18). Scripts: `mount`, list each name as bytes with `od`, then `touch`
new names.

- **Reading the Mac's names**: Linux has no stand-in mapping on these drivers (only the CIFS client has one), so it
  shows the private-use characters as themselves: `a:b` lists as `61 ef 80 a2 62` (renders as `ab` with an invisible
  glyph), `trail.` as `trail` + U+F029. `CON` and `con.txt` list as themselves. So a stand-in is wrong on Linux too; a
  reserved name is only Windows' problem.
- **Writing**: both drivers refuse `:`, `?`, `*`, and a control character with `EINVAL`. vfat refuses a trailing space;
  exfat takes one. Both STRIP a trailing period: `touch dup.` beside an existing `dup` "succeeded" and the folder still
  held only `dup`, so a write to `dup.` lands on `dup`. `LPT1` and `nul.txt` are created as themselves.

## Whether the SMB specs forbid a reserved device name

No. MS-FSCC (v20251121, the 2025-11-21 release, read 2026-11-18) § 2.1.5.2 "Filename" forbids only `" \ / : | < > * ?`
and 0x00–0x1F, caps a name at 255 characters, and names no device names; § 2.1.5.1 reserves `.` and `..` as path
components that MUST NOT be sent over the wire. MS-SMB2 carries names as MS-FSCC defines them. The device names are a
Win32 namespace rule: "Naming Files, Paths, and Namespaces" (learn.microsoft.com, updated 2025-04-11) says not to use
`CON` … `LPT³`, "Also avoid these names followed immediately by an extension; for example, NUL.txt and NUL.tar.gz are
both equivalent to NUL". Windows 11 relaxed the extension forms: from its first release (21H2, build 22000) `con.txt`
opens as an ordinary file, while a bare `CON` still opens the console (Super User question 1929654, answered
2025-11-03). Windows 10 still treats `con.txt` as the device. So on a share Cmdr warns about every reserved name, bare
ones included, and refuses none; the wire-forbidden characters never cross raw, since smb2 and smbfs send their
private-use stand-ins.

## What Cmdr does with it

Two tiers, typed (`cmdr_fs::validation::NameTier`): a refusal only where the destination itself refuses the name or
would lose data taking it, and a portability warning where it takes the name but won't keep it portable.

- **Every destination refuses**: an empty name or one that renders as nothing (only whitespace and invisible
  characters), `/`, NUL, `.` and `..`, and a name past the filesystem's own length limit in its own unit.
- **exFAT, FAT32, NTFS written from a Mac**: warn about the stand-in characters, control characters, a trailing space or
  period ("won't work on Windows or Linux"), and the reserved names ("won't work on Windows").
- **The same from Linux**: refuse the characters, control characters, and a trailing space or period (the driver refuses
  them, or strips the period onto a sibling); warn about the reserved names.
- **The same from Windows**: refuse all three (Win32 refuses the characters, strips the trailing ones, and opens a
  device for a reserved name on Windows 10).
- **An SMB share, from any host**: warn about all three ("won't work on Windows"; a reserved name may also list under
  another name, as Samba does).
- **Everything else**: POSIX names, with no warnings.
