# Typography and motion on Windows (task 62)

The evidence behind `apps/desktop/src/app-type-motion-windows.css`: the Windows look's font, type ramp, and motion
tokens, and what follows the OS on its own. The tokens' shape and where they live: `apps/desktop/src/DETAILS.md` §
Control look tokens.

## The sources

`microsoft/microsoft-ui-xaml` at commit `8c4a489bac3da36224ba325453146217619fe919` (main, 2026-11-18), the same pin as
`control-look-tokens.md` and `menu-look-tokens.md`:

- `controls/dev/CommonStyles/TextBlock_themeresources.xaml`: the ramp. `CaptionTextBlockFontSize` 12,
  `BodyTextBlockFontSize` 14, `BodyLargeTextBlockFontSize` 18, `SubtitleTextBlockFontSize` 20, `TitleTextBlockFontSize`
  28, `TitleLargeTextBlockFontSize` 40, `DisplayTextBlockFontSize` 68. `BaseTextBlockStyle` sets `FontFamily`
  `XamlAutoFontFamily`, `LineStackingStrategy` `MaxHeight`, and no `LineHeight`, so WinUI's leading is the font's own.
  Body, Caption, and BodyLarge are `Normal` weight; BodyStrong, Subtitle, and every larger step `SemiBold`.
- `controls/dev/CommonStyles/Common_themeresources_any.xaml`: `ControlFasterAnimationDuration` 83 ms,
  `ControlFastAnimationDuration` 167 ms, `ControlNormalAnimationDuration` 250 ms, `ControlFastOutSlowInKeySpline`
  `0,0,0,1`.
- `controls/dev/CommonStyles/Button_themeresources.xaml`: every button state's background change is a
  `BrushTransition Duration="0:0:0.083"`, which takes no easing. `ToggleSwitch_themeresources.xaml`: the knob's hover
  grow runs on `ControlFasterAnimationDuration`, its slide on `ControlNormalAnimationDuration`, both on the
  fast-out-slow-in spline.

## The font

- **Decision**: `'Segoe UI Variable Text', 'Segoe UI', system-ui, sans-serif`, as `--font-system` under the Windows
  look, so every surface that reads the token (and, through `html, body`, every one that inherits) wears it. The menus'
  `--font-menu-family` reads the same token.
- **Why the Text instance**: Chromium doesn't resolve the variable font's family name, `'Segoe UI Variable'`, only its
  named instances `Segoe UI Variable Small`, `Text`, and `Display` (Tiger Oakes, Edge team, "How to use Segoe UI
  Variable in a website", tigeroakes.com, read 2026-11-19; Microsoft's Windows 11 font list names the same instances).
  Text is the instance WinUI's body sizes land closest to.
- **Why Segoe UI second**: Windows 10 ships Segoe UI only (D5 ruled one look on both). `system-ui` (Chromium's message
  font, Segoe UI unless the user changed it) and `sans-serif` close the stack.
- **Respecting the system font**: Windows has no user-facing system font setting, and WinUI ignores the message font
  anyway (`XamlAutoFontFamily` always picks Segoe UI Variable), so naming the font matches native apps. Text size is the
  setting that matters, and WebView2 applies it itself: Chromium multiplies `UISettings.TextScaleFactor` into the device
  scale factor and follows it live (crbug 40586200, WebView2Feedback #1662: a per-monitor-DPI-aware host, which tao
  makes Cmdr, zooms with Text size as Edge does), so task 40's backend reports `1.0` into `--font-scale` rather than
  scaling twice. The trade: the whole page zooms, where WinUI grows text alone.
- **Gap: optical size**. `XamlAutoFontFamily` sets the `opsz` axis from the font size. In a browser each named instance
  has its own fixed `opsz` (Small 8, Text 10.5, Display 36), and the same article reports `font-optical-sizing` doesn't
  vary it. So every size draws the Text cut: a 20px dialog title is a touch looser than WinUI's. Picking `Display` per
  heading would need a second token in components; unverified whether WebView2 today varies `opsz` on its own.
- **Unverified on a real machine**: that WebView2 resolves `'Segoe UI Variable Text'` (2026-11-19, no Windows box). The
  Mac shots fall back to the system font, as this Mac has no Segoe UI.

## The type ramp

- `--font-size-sm` 12 is Caption, `--font-size-md` 14 Body, `--font-size-xl` 20 Subtitle: already on the ramp, so they
  stay.
- **Decision**: `--font-size-lg` (the house's 16px dialog and section titles) becomes BodyLarge 18 under the Windows
  look: the ramp has no 16, and 18 is its step between Body and Subtitle. The dialog title already takes Subtitle 20
  (`--font-dialog-title-size`, task 60).
- `--font-size-xs` 10 (badges, shortcut hints) has no ramp step and stays: Caption 12 would break the dense rows it sits
  in.
- **Not carried: weights**. WinUI's headings are SemiBold (600); stylelint's `font-weight` allowlist takes no `var()`,
  and Cmdr's headings are already 600 (`SettingsSection`, `ModalDialog`'s title).
- **Not carried: leading**. WinUI uses the font's metrics; Cmdr's leading tokens (`app.css` § Leading) stay, since the
  main window already uses the font's metrics and the text surfaces' 1.4 is close to Segoe UI's own.

## Motion

- **Decision**: `--transition-fast` and `--transition-base` are `83ms linear` (the `BrushTransition` every control state
  change runs on); `--transition-slow` (fades and size changes) is `250ms cubic-bezier(0, 0, 0, 1)`
  (`ControlNormalAnimationDuration` on `ControlFastOutSlowInKeySpline`).
- The roughly 40 raw durations in components (pulses, shakes, the toast's slide, column-width animations) are Cmdr's own
  feedback with no WinUI twin, and stay the same on every look.
- **Gap: WinUI's theme animations** (the flyout's `MenuPopupThemeTransition`, the switch knob's grow and stretch, the
  `ContentDialog` open scale) live in WinUI's C++ animation code; the menus and the dialog open without them, as before.

## The system's animation setting

- Windows' **Animation effects** (Settings > Accessibility > Visual effects, `SPI_GETCLIENTAREAANIMATION`) reaches the
  page as `prefers-reduced-motion: reduce` through Chromium (MDN's `prefers-reduced-motion` § User preferences names the
  setting, read 2026-11-19), so the `@media (prefers-reduced-motion)` rules Cmdr already has cover Windows with no
  backend reader. Unverified in WebView2 on a real machine, and so is the live update when the toggle flips.
- **Linux**: MDN lists GNOME's Reduced animation (`gtk-enable-animations`) and KDE's Animation speed set to Instant as
  the settings behind the same query, and the house stack's `system-ui` is the desktop's font. Linux keeps the house
  look, so nothing here changes it. Unverified that WebKitGTK honors either, on a Linux desktop.

## Proof that macOS and Linux didn't change

The browser demo built before and after (`pnpm build:demo`), shot in headless Chromium for a Mac visitor in light and
dark (the main pane, a right-click menu, the New folder dialog, and Settings): 0 differing pixels in all eight
(`magick compare -metric AE`, 2026-11-19). The only change that reaches the house look is `html, body` reading
`font-family: var(--font-system)`, whose house value is the stack `app.html` already wrote. `app-controls.test.ts`
snapshots the house font, sizes, and transitions.
