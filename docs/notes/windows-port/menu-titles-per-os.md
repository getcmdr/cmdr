# Menu titles per OS

Off macOS the page draws the menu bar (task 63, D5), but its titles and rows still read the catalog's plain `menu.*`
values, which several locales translated with the German, Dutch, Swedish, ... macOS words. A German Windows user sees
"Ablage" and "Darstellung" where every Windows app says "Datei" and "Ansicht". Found in the review of #1281
(2026-11-19); not fixed there, since it touches every locale.

## Why it happens

- The English source is plain for these keys (`menu.bar.file` is "File" on every OS), so a translator gets one value.
- `docs/guides/i18n-translation.md` (rule 2) says a plain value takes the word the OSes share, else the macOS Finder
  term, and `docs/i18n/de/style.md` § "Native menus and Apple names" rules the macOS words for the bar (`Ablage`,
  `Darstellung`, `Widerrufen`, `Einsetzen`, `Im Dock ablegen`): right while only macOS had a Cmdr-drawn bar, wrong now
  that Windows and Linux draw one from the same keys.
- Per-OS values already work end to end: `menu.file.quit` and `menu.edit.undoTrash` are `{platform, select, ...}` in
  English, every locale fills their branches, and Rust's `menu_t` resolves them for the native bar
  (`native_strings.gen.rs`).

## What's needed

1. Make the English of each affected key a per-OS select with the same English in every branch
   (`{platform, select, macos {File} other {File}}`), so translators get a branch per OS. The keys are the titles
   (`menu.bar.file`, `menu.bar.edit`, `menu.bar.select`, `menu.bar.view`, `menu.bar.go`, `menu.bar.tab`,
   `menu.bar.window`, `menu.bar.help`) and the rows whose macOS word differs from Windows' in some locale
   (`menu.edit.undo`, `menu.edit.redo`, `menu.edit.paste`, `menu.window.minimize`, `menu.window.zoom`). Check each key's
   other uses first: `menu.bar.*` also titles the viewer bar (`cmdr_menu::page::viewer`) and the main-menu button's
   submenus, and some keys are quoted in Settings or help copy (`term-consistency`, the quote pairing in
   `i18n-translation.md`).
2. Run a translator agent per locale (the guide's process), each filling the `windows` branch from
   `microsoft-terminology/` and `other` from the GNOME, KDE, and Xfce catalogs (`docs/i18n/translator-instructions.md` §
   Per-OS values), leaving the `macos` branch as it is.
3. Update each locale's `style.md` and `terms.json` (the menu-bar exceptions, for example de `file`'s `menu.bar.file`
   exception) so they say the macOS words are the `macos` branch's, and drop the now-split keys from
   `docs/i18n/source-queue.md`.
4. Re-run `pnpm intl:native-strings`, the i18n checks, and the menu bar snapshots; capture the Windows and Linux bars in
   each language (`docs/guides/screenshots.md`).

## Where it shows today

The plain values that look macOS-only in today's catalogs (from `apps/desktop/src/lib/intl/messages/*/menu.json`,
2026-11-19). These are candidates for the translators to confirm from the pile, not rulings:

- **de**: `Ablage` (File), `Darstellung` (View), `Widerrufen` (Undo), `Einsetzen` (Paste), `Im Dock ablegen` (Minimize),
  `Zoomen` (Zoom); Windows says `Datei`, `Ansicht`, `Rückgängig`, `Einfügen`.
- **nl**: `Archief` (File), `Wijzig` (Edit), `Herstel` (Undo), `Plak` (Paste); Windows says `Bestand`, `Bewerken`,
  `Ongedaan maken`, `Plakken`.
- **sv**: `Innehåll` (View); Windows says `Visa`. `Arkiv` (File) is Windows' word too.
- **fr**: `Présentation` (View); Windows says `Affichage`.
- **es**: `Visualización` (View); Windows says `Ver`.
- **pt**: `Visualizar` (View); Windows says `Exibir`.
- **cs**: `Odvolat akci` (Undo), `Přepnout velikost` (Zoom); Windows says `Zpět`.
- **ru**: `Убрать в Dock` (Minimize), a macOS-only word on any non-Mac bar.
- **uk**: `Оптимізувати` (Zoom).
- **zh**: `显示` (View); Windows says `查看`.
- **zh-Hant**: `顯示方式` (View), `輔助說明` (Help), `縮到最小` (Minimize); Windows says `檢視`, `說明`, `最小化`.
- **hu** and **vi** read as OS-neutral already; confirm.

The overlays (`en-GB`, `en-AU`, `es-419`) follow their base catalogs.
