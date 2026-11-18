# Platform conventions: the guidelines behind them

Evidence for task 59 of `docs/specs/windows-port-plan.md`: the Phase 7 "starting assumptions" checked against each
platform's own guidelines, and the source of every value in `PlatformConventions`
(`apps/desktop/src-tauri/src/platform/conventions.rs`). Everything marked verified was read from the page on 2026-11-18
(Firecrawl scrape, quoted text); the page's own date is given where it shows one. "Unverified" means from memory or
secondary sources, not confirmed on a primary page.

## The sources

- **Apple HIG, Alerts** (`developer.apple.com/design/human-interface-guidelines/alerts`, change log to 2024-02-02):
  "Always place the default button on the trailing side of a row or at the top of a stack. Cancel buttons are typically
  on the leading side of a row." Cancel by "Pressing Escape (Esc) or Command-Period (.) on an attached keyboard" (macOS
  listed). Don't make Cancel the default; with a destructive action nobody chose, consider no default at all. Read again
  for task 61 (2026-11-18, Firecrawl scrape, page change log to 2024-02-02 "Enhanced guidance for using default and
  Cancel buttons"), quoted: "Note that you don't want to make a Cancel button the default button. If you want to
  encourage people to read an alert and not just automatically press Return to dismiss it, avoid making any button the
  default button." And on the destructive style: "when people deliberately choose a destructive action — such as Empty
  Trash — the resulting alert doesn't apply the destructive style to the Empty Trash button because the button performs
  the person's original intent. In this scenario, the convenience of pressing Return to confirm the deliberately chosen
  Empty Trash action outweighs the benefit of reaffirming that the button is destructive."
- **Apple HIG, The menu bar** (`…/the-menu-bar`, change log to 2025-06-09): "On a Mac or an iPad, the menu bar at the
  top of the screen displays the top-level menus in your app."
- **Microsoft, Windows 7 dialog boxes (Win32 UX guide)**
  (`learn.microsoft.com/en-us/windows/win32/uxguide/win-dialog-box`, `ms.date` 2022-01-25): "Right-align commit buttons
  in a single row across the bottom of the dialog box", in the order "OK/[Do it]/Yes, [Don't do it]/No, Cancel, Apply
  (if present), Help (if present)". "Don't assign access keys to Cancel, because Esc is the access key" (same for
  Close).
- **Microsoft, Dialog controls (WinUI `ContentDialog`)**
  (`learn.microsoft.com/en-us/windows/apps/develop/ui/controls/dialogs-and-flyouts/dialogs`, last updated 2026-07-15):
  PrimaryButton "Appears as the leftmost button", SecondaryButton the middle, CloseButton "the safe, nondestructive
  action … Appears as the rightmost button", pressed by Esc. A DefaultButton responds to Enter, "If the user has placed
  Keyboard Focus on a control that handles ENTER, the Default Button will not respond", and no default is set unless
  chosen.
- **Microsoft, Access keys design guidelines** (`learn.microsoft.com/en-us/windows/apps/develop/input/access-keys`,
  `ms.date` 2024-06-11): "a combination of the Alt key and one or more alphanumeric keys", shown in keytips "when the
  user presses the Alt key". "We recommend that you specify access keys wherever appropriate in your UI."
- **Microsoft, Menu flyout and menu bar** (`learn.microsoft.com/en-us/windows/apps/develop/ui/controls/menus`, last
  updated 2026-10-14): a menu bar shows "a set of multiple top-level menus in a horizontal row … typically position[ed]
  at the top of the app window". It offers the control and doesn't prescribe it over a command bar.
- **GNOME HIG, Dialogs** (`developer.gnome.org/hig/patterns/feedback/dialogs.html`): "Always ensure that the cancel
  button appears first, before the affirmative button. In left-to-right locales, this is on the left." Return activates
  the affirmative button unless it's "irreversible, destructive or otherwise inconvenient"; "Ensure that the Esc key
  activates the cancel button."
- **GNOME HIG, Keyboard** (`…/guidelines/keyboard.html`): access keys are Alt plus an underlined letter, "displayed when
  Alt is held down", and "As far as possible, all labelled components should have an access key." "Do not use Alt for
  shortcut keys, as this may conflict with access keys." Super is reserved for the system.
- **GNOME HIG, Menus** (`…/patterns/controls/menus.html`): the app's menu is a primary menu (the "Main Menu" button) at
  the end of the header bar; it holds no Close or Quit. The HIG describes no menu bar.
- **KDE HIG, Layout and navigation** (`develop.kde.org/hig/layout_and_nav/`, modified 2026-11-12): a hamburger menu of
  about 15 items or fewer, else "place a real menubar between the titlebar and toolbar"; `KHamburgerMenu` gives a larger
  app both.
- **KDE HIG, Text and labels** (`develop.kde.org/hig/text_and_labels/`, modified 2026-11-12): accelerator keys are
  written "only for text in buttons, radio buttons, checkboxes, and switches (in other places, they are
  auto-generated)", so KDE apps carry access keys everywhere.
- **KDE HIG, Accessibility** (`develop.kde.org/hig/accessibility/`, modified 2026-09-01): test "with the Orca screen
  reader".
- **KDE button order**: the KDE HIG (and its "Getting input" page, modified 2026-11-08, on dialogs) doesn't state an
  order. KDE apps get theirs from Qt: `qtbase` `src/gui/kernel/qplatformdialoghelper.cpp` (`dev` branch, read
  2026-11-18), `buttonRoleLayouts`, `KdeLayout` = Help, Reset, stretch, Yes, No, Action, Accept, Alternate, Apply,
  Destructive, Reject, the same shape as `WinLayout` (Accept before Reject, right-aligned). `GnomeLayout` and
  `MacLayout` reverse it (Reject before Accept).
- **WebAIM Screen Reader User Survey #11** (`webaim.org/projects/screenreadersurvey11/`, 2026): primary desktop screen
  reader JAWS 55.0%, NVDA 32.9%, Orca 1.4%, Narrator 0.4%; commonly used JAWS 69.2%, NVDA 59.3%, Narrator 36.1%, Orca
  6.5%. Survey #10 (2024) had JAWS 40.5%, NVDA 37.7%.
- **Reserved shortcuts per desktop**: task 26 read them from each desktop's own source (GNOME Shell, mutter,
  gnome-settings-daemon, KWin, Plasma, Xfce, Cinnamon, MATE): `keymap-decisions.md` § the reserved-keys survey. Not
  repeated here.

## The Phase 7 table, checked

- **Shortcuts: Windows (Ctrl-based), minus what GNOME and KDE reserve**: holds. GNOME's standard shortcuts are
  Ctrl-based and Super is the system's; task 26 shipped this with the per-desktop reserved keys. Addition: GNOME's HIG
  asks apps not to bind Alt+letter shortcuts, which it keeps for access keys. Cmdr's Linux (and Windows) defaults bind
  14 `Alt+<letter>` keys, all scoped to one surface's controls (the multi-rename dialog's options, the viewer's search
  toggles, Stop in About this), so they already work as access keys by hand; task 61 should make them the dialogs'
  access keys rather than a second mechanism beside them (checked against
  `apps/desktop/src/lib/commands/__snapshots__/default-keymap.linux.txt`, 2026-11-18). Task 61 did, for the 12 inside
  dialogs: they keep their commands and reserve their letters in their dialog's access keys
  (`apps/desktop/src/lib/ui/DETAILS.md` § Platform conventions).
- **Window chrome: Windows (native decorations)**: holds as a decision; no guideline disagrees. GNOME draws decorations
  client-side (a header bar), which a GTK window gets natively.
- **Paths, Trash, permissions: macOS (POSIX)**: not a guideline question (the freedesktop.org Trash spec and POSIX);
  unchanged.
- **Dialog button order: GNOME like macOS, KDE like Windows**: holds, now verified (GNOME HIG; Qt's `KdeLayout` for KDE,
  since the KDE HIG is silent). Other desktops follow GNOME's order, a decision: most are GTK-based, and Cmdr's own
  window is GTK (WebKitGTK), so it matches the system dialogs the app shows.
- **Control styling: its own neutral theme**: holds as a decision. Neither HIG asks third-party apps to copy Adwaita or
  Breeze; GNOME and KDE each describe their own toolkit's look.
- **Words: its own, desktop-specific where GNOME and KDE differ**: holds. The desktop is now a backend value
  (`linuxDesktop`), so copy can split on it once the catalog can select on it (see "Left").
- **Screen reader: its own (Orca)**: holds (KDE HIG names Orca; GNOME's screen reader is Orca, unverified on a GNOME
  page). Correction for Windows: task 64 named Narrator and NVDA, and JAWS leads both in the 2026 survey (55% primary).
  The Windows row expects JAWS, NVDA, and Narrator, most used first.
- **Menu model** (task 63's question, not in the table): the guidelines disagree with today's Linux bar on GNOME, whose
  HIG has no menu bar at all. KDE accepts a menu bar for an app Cmdr's size. Windows offers both a menu bar and a
  command bar without preferring one.

## The values and their evidence

- `dialogButtonOrder`: macOS and GNOME `cancelFirst`, Windows and KDE `affirmativeFirst` (verified, sources above).
  Other Linux: `cancelFirst` (decision).
- `enterActivates`: macOS `defaultButton`: AppKit sends Return to the default button wherever focus is, and Space
  presses the focused one (unverified on an Apple page; the HIG says which button should be the default, and that Return
  presses it, but not what a focused button does). Windows `focusedButton`: WinUI's DefaultButton yields Enter to a
  focused control that handles it (verified), and in Win32 a focused push button takes over as the default (unverified
  on a primary page). GNOME, KDE, and other Linux `focusedButton`: GTK and Qt buttons activate on Enter when focused,
  and Qt's `autoDefault` makes a focused dialog button the default (unverified on a primary page).
- `cancelKeys`: macOS `escapeOrCommandPeriod` (verified, Apple HIG Alerts); Windows and Linux `escape` (verified: Win32
  UX guide, WinUI `ContentDialog`, GNOME HIG; KDE via Qt's `QDialog`, unverified on a primary page).
- `accessKeys`: macOS `none` (no mnemonics on the Mac, unverified on an Apple page). Windows `keytips` (verified,
  Microsoft's access keys guide: keytips when Alt is pressed; Win32 underlines are the older look, a decision for
  Windows 11's). GNOME, KDE, and other Linux `underlines` (verified for GNOME: underlined letter "displayed when Alt is
  held down"; KDE's underline-on-Alt is Breeze's default with a setting to always show them, unverified on a primary
  page). A CJK label's mnemonic as "保存(_S)", the letter in parentheses after the label, is GTK and Qt translators'
  practice (unverified on a primary page); Cmdr appends " (S)" itself, since its letters are picked from rendered
  labels.
- How task 61 applied `enterActivates` on the Mac: the HIG's "no default button" for a confirmation people should read
  (the quit prompt, the tool question, the rollback question, the data wipe), and Return keeps confirming a deliberately
  chosen destructive action (permanent delete, Delete AI model, Forget memory), as the Empty Trash passage allows. One
  deliberate departure from AppKit for data safety: while the default button is destructive, Return on another footer
  button does nothing, where AppKit would press the default. The table: `apps/desktop/src/lib/ui/DETAILS.md` § Platform
  conventions.
- `menuModel`: macOS `systemMenuBar` (verified). Windows and every Linux desktop `windowMenuBar`, carrying today's bar:
  David decides the Windows model in task 63, which also revisits GNOME.
- `screenReaders`: macOS `voiceOver`; Windows `jaws`, `nvda`, `narrator` (verified, WebAIM #11); Linux `orca` on every
  desktop (verified for KDE).
- `linuxDesktop`: from `XDG_CURRENT_DESKTOP` (the freedesktop.org desktop-entry spec's registered names,
  colon-separated, most specific first; unverified on the spec page this time): the first `GNOME` or `KDE` entry
  decides, so Ubuntu's `ubuntu:GNOME` and Budgie's `Budgie:GNOME` read as GNOME. Anything else is `other`.
