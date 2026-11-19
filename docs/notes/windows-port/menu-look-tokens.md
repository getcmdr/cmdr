# Menu look tokens: where each Windows value comes from

Evidence for task 63 step 3 of `docs/specs/windows-port-plan.md` (D5: Windows 11's look, matched precisely, on Windows
10 and 11). The tokens live in `apps/desktop/src/app-menu.css`; how they're keyed and cascaded is in
`apps/desktop/src/DETAILS.md` § Menu look tokens. This page is the source list and what CSS couldn't carry.

## The sources

Every WinUI file was read at `microsoft/microsoft-ui-xaml` commit `8c4a489bac3da36224ba325453146217619fe919` (`main`,
committed 2026-11-18T17:27Z), fetched with `gh api` on 2026-11-18:

- `controls/dev/CommonStyles/MenuFlyout_themeresources.xaml`: `MenuFlyoutPresenter`, `MenuFlyoutItem`,
  `ToggleMenuFlyoutItem`, `MenuFlyoutSubItem`, `MenuFlyoutSeparator`, and their brushes and sizes.
  `MenuFlyout_themeresources_perf2026.xaml` (the `ThemeResourcesPerf2026` build of the same) differs only in swapping
  the narrow-padding `Storyboard`s for `VisualState.Setters`; every value is the same, so either file answers.
- `controls/dev/RadioMenuFlyoutItem/RadioMenuFlyoutItem_themeresources.xaml`: the radio bullet (same 12px glyph,
  `MenuFlyoutSubItemChevron` color, 16px margin as the check).
- `controls/dev/MenuBar/MenuBar_themeresources.xaml`, `MenuBar.xaml`, `MenuBarItem.xaml`: the bar and its titles.
- `controls/dev/CommonStyles/CornerRadius_themeresources.xaml`: `ControlCornerRadius` 4, `OverlayCornerRadius` 8 (the
  same in every theme).
- `controls/dev/CommonStyles/Common_themeresources_any.xaml`: the color ramp (`TextFillColor*`, `SubtleFillColor*`,
  `SurfaceStrokeColorFlyout`, `DividerStrokeColorDefault`, `FocusStrokeColor*`), per theme, and the `HighContrast`
  mapping of each brush to a `SystemColor…Color`.
- `controls/dev/Materials/Acrylic/AcrylicBrush_themeresources.xaml`, `AcrylicBrush.h`, `AcrylicBrush.cpp`: the acrylic
  recipe.
- `controls/dev/CommonStyles/TextBlock_themeresources.xaml`: the type ramp (`CaptionTextBlockFontSize` 12,
  `BodyTextBlockFontSize` 14).
- `dxaml/xcp/dxaml/themes/generic.xaml`: `ControlContentThemeFontSize` 14, `FlyoutThemeMinWidth` 96,
  `ContentControlThemeFontFamily` (`XamlAutoFontFamily`), and the `HighContrast` `SystemControl…` brushes the bar uses.
- `dxaml/xcp/dxaml/lib/MenuFlyoutItem_Partial.cpp` (`ChangeVisualState`): which state a row is in. Keyboard focus is its
  own `Focused` state, separate from `PointerOver`, and the item template has no `FocusStates` group, so a focused row
  shows the system focus visual and no fill.

Two values aren't in the WinUI repo and come from Microsoft's other public sources:

- **The flyout shadow.** WinUI draws it with `ThemeShadow` at the flyout's elevation, which the compositor renders. The
  elevation is 32 (`learn.microsoft.com/en-us/windows/apps/design/signature-experiences/layering`, last updated
  2026-02-19, verified 2026-11-18 by Firecrawl scrape: "Flyout, Elevation value: 32, Stroke width: 1"). The CSS is
  Fluent's own web token for that height, `shadow28`, from `microsoft/fluentui` commit
  `feab6ba63b55703d803f17092eab54c9dc82430b`: `packages/tokens/src/utils/shadows.ts`
  (`0 0 8px ambient, 0 14px 28px key`), with `alias/lightColor.ts` (ambient 0.12, key 0.14) and `alias/darkColor.ts`
  (0.24, 0.28).
- **Which acrylic menus use.** `learn.microsoft.com/en-us/windows/apps/design/style/acrylic` (`ms.date` 2026-10-02,
  verified 2026-11-18): "MenuFlyout … and similar controls with light-dismiss popups all use acrylic while open", and
  background acrylic for menus that can draw outside the window.

## The tokens

Each Windows token, its WinUI resource key, and the resolved value (WinUI writes ARGB, CSS `#rrggbbaa`; light / dark /
contrast themes). WinUI's `Default` theme dictionary is the dark theme.

Surface:

- `--color-menu-surface-bg`: the acrylic luminosity layer. `MenuFlyoutPresenterBackground` is
  `DesktopAcrylicTransparentBrush` (`#00000000`) over `MenuFlyoutSystemBackdrop` →
  `AcrylicBackgroundFillColorDefaultBackdrop` (a `DesktopAcrylicBackdrop`), whose in-app twin
  `AcrylicBackgroundFillColorDefaultBrush` carries the recipe: light `TintColor` `#FCFCFC`, `TintLuminosityOpacity` 0.85
  → `#fcfcfcd9`; dark `#2C2C2C` at 0.96 → `#2c2c2cf5`; contrast `SystemColorWindowColor` → `Canvas`.
- `--color-menu-surface-tint`: the tint layer, `TintColor` at `TintOpacity` (folded into the alpha as
  `AcrylicBrush::GetEffectiveTintColor` does, since `TintLuminosityOpacity` is set): light 0 → `#fcfcfc00`; dark 0.15 →
  `#2c2c2c26`; contrast none.
- Reduced transparency: `FallbackColor`, light `#F9F9F9`, dark `#2C2C2C`, opaque.
- `--glass-menu-backdrop`: `AcrylicBrush.h` `sc_blurRadius` 30 → `blur(30px)` (a Gaussian standard deviation, which is
  what CSS `blur()` takes); none under reduced transparency and contrast themes.
- `--color-menu-surface-border`: `MenuFlyoutPresenterBorderBrush` → `SurfaceStrokeColorFlyoutBrush`: light `#0F000000`,
  dark `#33000000`, contrast `SystemColorWindowTextColor` → `CanvasText`.
- `--spacing-menu-surface-border`: `MenuFlyoutPresenterBorderThemeThickness` 1, contrast 2.
- `--radius-menu-surface`: `OverlayCornerRadius` 8.
- `--shadow-menu-surface`: Fluent `shadow28` (above); contrast none.
- `--spacing-menu-surface-padding-block`: `MenuFlyoutPresenterThemePadding` 0,2,0,2.
- `--spacing-menu-surface-min-width`: `FlyoutThemeMinWidth` 96 (the presenter's `MinWidth`; it sets no max width).

Row:

- `--spacing-menu-row-margin-block` / `-inline`: `MenuFlyoutItemMargin` 4,2,4,2.
- `--spacing-menu-row-padding-top` / `-bottom` / `-start` / `-end`: `MenuFlyoutItemThemePaddingNarrow` 11,4,11,5. WinUI
  picks the narrow padding when a menu opens by mouse, pen, or keyboard (`PaddingSizeStates`, `GetShouldBeNarrow`) and
  `MenuFlyoutItemThemePadding` 11,8,11,9 for touch. Cmdr's menus open by mouse or keyboard, so the narrow one.
- `--radius-menu-row`: `ControlCornerRadius` 4.
- `--font-menu-family`: `ContentControlThemeFontFamily` → `XamlAutoFontFamily`, Segoe UI Variable on Windows 11 (Segoe
  UI on Windows 10), through the app's `--font-system`.
- `--font-menu-size`: `ControlContentThemeFontSize` 14 (`--font-size-md`, so it follows Cmdr's text size).
- `--font-menu-line-height`: `normal`, the font's own metrics, as a XAML `TextBlock` without a `LineHeight` uses.
- `--color-menu-row-fg` / `-fg-hover`: `MenuFlyoutItemForeground` and `…PointerOver` / `…Pressed` →
  `TextFillColorPrimary`: light `#E4000000`, dark `#FFFFFF`; contrast rest `SystemColorButtonTextColor` → `ButtonText`,
  hover and pressed `SystemColorHighlightTextColor` → `HighlightText`.
- `--color-menu-row-fg-disabled`: `MenuFlyoutItemForegroundDisabled` → `TextFillColorDisabled`: light `#5C000000`, dark
  `#5DFFFFFF`, contrast `GrayText`.
- `--color-menu-row-bg-hover`: `MenuFlyoutItemBackgroundPointerOver` → `SubtleFillColorSecondary`: light `#09000000`,
  dark `#0FFFFFFF`, contrast `Highlight`.
- `--color-menu-row-bg-pressed`: `MenuFlyoutItemBackgroundPressed` → `SubtleFillColorTertiary`: light `#06000000`, dark
  `#0AFFFFFF`, contrast `Highlight`.
- `--color-menu-row-bg-open` (a submenu row while its submenu is open): `MenuFlyoutSubItemBackgroundSubMenuOpened` →
  `SubtleFillColorSecondary`; contrast transparent, where the label turns `Highlight` instead
  (`MenuFlyoutSubItemForegroundSubMenuOpened`).
- `--spacing-menu-glyph-column`: `MenuFlyoutItemPlaceholderThemeThickness` 28,0,0,0, the label's inset in a menu holding
  a check, radio, or icon (a 12px glyph plus a 16px margin; `MenuFlyoutItemDoublePlaceholderThemeThickness` 56 when it
  holds both, which step 4 can derive as twice the column).

Shortcut (WinUI's keyboard accelerator text):

- `--font-menu-shortcut-size`: `KeyboardAcceleratorTextBlock` `Style` → `CaptionTextBlockStyle` →
  `CaptionTextBlockFontSize` 12.
- `--spacing-menu-shortcut-gap`: `KeyboardAcceleratorTextBlock` `Margin` 24,4,0,0 (the 4px top nudges it toward the
  label's baseline; 24,0,0,0 on toggle rows).
- `--color-menu-shortcut-fg` / `-fg-hover`: `MenuFlyoutItemKeyboardAcceleratorTextForeground` and `…PointerOver` →
  `TextFillColorSecondary`: light `#9E000000`, dark `#C5FFFFFF`; contrast rest `SystemColorWindowTextColor` →
  `CanvasText`, hover `HighlightText`.
- `--color-menu-shortcut-fg-disabled`: `…Disabled` → `TextFillColorDisabled`; contrast `GrayText`.

Glyphs:

- `--color-menu-check-fg`: `ToggleMenuFlyoutItem` and `RadioMenuFlyoutItem` `CheckGlyph` → `MenuFlyoutSubItemChevron` →
  `TextFillColorSecondary`; on hover and press it takes the row's foreground (`MenuFlyoutItemForegroundPointerOver`,
  `MenuFlyoutSubItemForegroundPressed`), so the component uses `--color-menu-row-fg-hover` there. Contrast `ButtonText`.
  The glyphs are Segoe Fluent Icons `E73E` (check) and `E915` (bullet) at 12px; Cmdr draws its own icons at that size.
- `--color-menu-chevron-fg` / `-fg-pressed`: `MenuFlyoutSubItemChevron` (`E974`, 12px, `MenuFlyoutItemChevronMargin`
  24,0,0,-1) → `TextFillColorSecondary`, `…Pressed` → `TextFillColorTertiary` (light `#72000000`, dark `#87FFFFFF`);
  contrast `ButtonText`, pressed `HighlightText`.

Separator:

- `--color-menu-separator`: `MenuFlyoutSeparatorBackground` → `DividerStrokeColorDefault`: light `#0F000000`, dark
  `#15FFFFFF`, contrast `CanvasText`.
- `--spacing-menu-separator-margin-block` and `--spacing-menu-separator-inset`: `MenuFlyoutSeparatorHeight` 1 with
  `MenuFlyoutSeparatorThemePadding` -4,1,-4,1. The presenter has no side padding, so the line runs edge to edge (inset
  0), with 1px above and below.

Keyboard focus:

- `--color-menu-focus-outer` / `-inner`, `--spacing-menu-focus-outer` / `-inner`: `UseSystemFocusVisuals`, drawn with
  `FocusStrokeColorOuter` (light `#E4000000`, dark `#FFFFFF`, contrast `CanvasText`) at the default
  `FocusVisualPrimaryThickness` 2, and `FocusStrokeColorInner` (light `#B3FFFFFF`, dark `#B3000000`, contrast `Canvas`)
  at `FocusVisualSecondaryThickness` 1. A focused row has no fill of its own (§ The sources, `ChangeVisualState`).

Menu bar:

- `--color-menu-bar-bg`: `MenuBarBackground` → `SubtleFillColorTransparent` (contrast `SystemControlTransparentBrush`).
- `--spacing-menu-bar-height`: `MenuBarHeight` 40. `--spacing-menu-bar-padding-inline`: 0 (the bar has none).
- `--spacing-menu-bar-item-height` and `--spacing-menu-bar-item-margin-inline`: `MenuBarItemMargin` 4,4,4,4, which
  leaves each title 32px tall.
- `--spacing-menu-bar-item-padding-block` / `-inline`: `MenuBarItemButtonPadding` 10,4,10,4.
- `--radius-menu-bar-item`: `ControlCornerRadius` 4.
- `--font-menu-bar-size`: the title is a `Button`, so `ControlContentThemeFontSize` 14.
- `--color-menu-bar-item-fg`: `MenuBarItemForeground` → `TextFillColorPrimary`; contrast `SystemColorButtonTextColor`.
- `--color-menu-bar-item-bg-hover`: `MenuBarItemBackgroundPointerOver` → `SubtleFillColorSecondary`.
- `--color-menu-bar-item-bg-pressed` / `-bg-open`: `MenuBarItemBackgroundPressed` and `…Selected` (its menu open) →
  `SubtleFillColorTertiary`. Contrast: all three `SystemColorButtonFaceColor` (`SystemControlBackgroundListLowBrush`,
  `…ListMediumBrush` in `generic.xaml`).
- `--spacing-menu-bar-item-border`, `--color-menu-bar-item-border` / `-hover`: `MenuBarItemBorderThickness` 0, except 2
  in contrast themes, where the border is `SystemColorButtonTextColor` at rest and `SystemColorHighlightColor` on hover,
  press, and open.

Added with the component (task 63 step 4), from the same sources:

- `--color-menu-row-bg-focus`, `--color-menu-row-fg-focus`, `--color-menu-shortcut-fg-focus`,
  `--color-menu-chevron-fg-focus`, `--color-menu-bar-item-bg-focus`: the keyboard's row and title. WinUI's `Focused`
  state sets no fill and keeps the rest colors (`MenuFlyoutItem_Partial.cpp`, § The sources), so Windows has
  `transparent` and the rest values, contrast `transparent` and `ButtonText` / `CanvasText`; the system focus visual
  (above) marks the row.
- `--color-menu-row-fg-open`: `MenuFlyoutSubItemForegroundSubMenuOpened` → `TextFillColorPrimary`, contrast
  `SystemColorHighlightColor` → `Highlight`.
- `--color-menu-chevron-fg-hover`: `MenuFlyoutSubItemChevronPointerOver` → `TextFillColorSecondary`, contrast
  `HighlightText`.
- `--spacing-menu-row-padding-start` / `-end` (the old `-inline` split, so a caller's look can differ by side, as the
  demo's macOS menus do), `--spacing-menu-row-min-height` (WinUI sets none: 0).
- `--spacing-menu-glyph-size` (12, the check and bullet glyph) and `--spacing-menu-glyph-column-empty` (0: WinUI's
  placeholder states apply only when a row holds a check, radio, or icon).
- `--font-menu-header-size`, `--color-menu-header-fg`, `--spacing-menu-header-min-height`: WinUI's menus have no header
  line; Cmdr's context menus lead with one, drawn as caption text (`CaptionTextBlockFontSize` 12) in
  `TextFillColorSecondary`, contrast `CanvasText`. Cmdr's own choice, for the look check.

## What CSS can't carry, and what the tokens do instead

- **The luminosity blend.** Acrylic blurs the backdrop, then a luminosity blend takes the luminosity from the tint color
  at `TintLuminosityOpacity`, then a color blend takes hue and saturation from the tint
  (`AcrylicBrush::CombineNoiseWithTintEffect_Luminosity`; its comment notes the two `BlendEffectMode` names are
  swapped). `backdrop-filter` has no blend step, so the tokens paint both layers source-over: the luminosity layer as
  the fill, the tint on top. With menus' near-neutral tints the difference is a slight loss of the backdrop's own color.
  A component could reproduce the blends with pseudo-elements in `mix-blend-mode: luminosity` and `color` over the
  `backdrop-filter`; step 4 can try it if the comparison shows the gap.
- **The noise.** Acrylic adds a tiled noise texture at `sc_noiseOpacity` 0.02. Left out: at 2% it's invisible next to
  the blur, and a data-URI texture adds paint cost for nothing visible.
- **What's blurred.** WinUI's flyout is its own window, so `DesktopAcrylicBackdrop` blurs the desktop behind the app;
  ours blurs the app beneath the menu (accepted in the spec's landmines).
- **The shadow.** `ThemeShadow` isn't CSS; Fluent's web `shadow28` stands in (§ The sources).
- **The font.** `XamlAutoFontFamily` picks Segoe UI Variable and sets its optical size from the font size. The menus
  read the app's `--font-system`, which the Windows look sets to Segoe UI Variable's Text instance; the stack and the
  optical-size gap: `docs/notes/windows-port/type-and-motion.md` § The font.
- **Motion.** None in these tokens: WinUI switches row and title states instantly (`VisualState.Setters`, no
  transitions). The flyout's open animation (`MenuPopupThemeTransition`) lives in WinUI's C++ animation code, so the
  menus open without it (`docs/notes/windows-port/type-and-motion.md` § Motion).
- **The contrast theme's colors.** WinUI maps each brush to a `SystemColor…Color`; CSS has the same set as system colors
  under `forced-colors: active`. Chromium forces colors on its own in that mode, so the bar and the menu surface set
  `forced-color-adjust` from the controls' `--color-forced-adjust` (`none` on the Windows look there) and paint these
  exact pairs (else a highlighted row's `Highlight` fill would be forced back to `Canvas`).

## Open questions for the design review

- Keyboard focus in a flyout: the source says a focused row gets the two-ring focus visual and no fill, and step 4 built
  exactly that (the `-focus` tokens above). Compare an arrowed-to row in Notepad's File menu with the reference shots;
  if Notepad fills it, `--color-menu-row-bg-focus` takes the hover fill and the rings stay or go.
- Where a flyout and a submenu sit: the flyout at the title's bottom-left on the bar's bottom edge, a submenu 5 px above
  its row with no overlap (`MenuSurface.svelte`'s `SUBMENU_OFFSET`, `menu-bar.svelte.ts`'s `FLYOUT_NUDGE`), from WinUI's
  paddings, not measured against Notepad.
- The submenu hover delay: 400 ms, Win32's default `MenuShowDelay`; WinUI's own delay is unverified.
- The font names WebView2 resolves for Segoe UI Variable (above).
