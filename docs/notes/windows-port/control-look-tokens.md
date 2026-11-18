# Control look tokens: where each Windows value comes from

Evidence for task 60 of `docs/specs/windows-port-plan.md` (a Windows theme for the `lib/ui` primitives, D5's rule:
Windows 11's look, matched precisely, on Windows 10 and 11). The tokens live in `apps/desktop/src/app-controls.css`
(house), `apps/desktop/src/app-controls-windows.css` (Windows light and dark, plus scrollbars), and
`apps/desktop/src/app-controls-windows-contrast.css` (contrast themes); how they're keyed is in
`apps/desktop/src/DETAILS.md` § Control look tokens. Each Windows token carries its resource key in a comment beside it,
so this page lists the sources, the derivations, and what CSS couldn't carry. The menu's own tokens and their sources:
`docs/notes/windows-port/menu-look-tokens.md`.

## The sources

Every WinUI file was read at `microsoft/microsoft-ui-xaml` commit `8c4a489bac3da36224ba325453146217619fe919` (`main`,
committed 2026-11-18T17:27Z, the commit `docs/notes/windows-port/menu-look-tokens.md` used), fetched with `gh api` on
2026-11-18. Where a file has a `_perf2026` twin, the values used here are the same in both.

- `controls/dev/CommonStyles/Common_themeresources_any.xaml`: the color ramp per theme (`TextFillColor*`,
  `ControlFillColor*`, `ControlAltFillColor*`, `ControlStrongFillColor*`, `ControlStrokeColor*`,
  `ControlStrongStrokeColor*`, `SubtleFillColor*`, `AccentFillColor*`, `FocusStrokeColor*`, `CardStrokeColorDefault`,
  `SurfaceStrokeColorDefault`, `SmokeFillColorDefault`, `SolidBackgroundFillColorBase`, `LayerFillColorAlt`,
  `SystemFillColorCritical`), the elevation border gradients (`ControlElevationBorderBrush`,
  `AccentControlElevationBorderBrush`, `CircleElevationBorderBrush`), and each brush's `HighContrast` system color.
- `controls/dev/CommonStyles/Common_themeresources.xaml`: `TextControlThemePadding` 10,5,6,6,
  `TextControlBorderThemeThickness` 1, `TextControlBorderThemeThicknessFocused` 1,1,1,2 (2 in high contrast).
- `controls/dev/CommonStyles/CornerRadius_themeresources.xaml`: `ControlCornerRadius` 4, `OverlayCornerRadius` 8.
- `controls/dev/CommonStyles/Button_themeresources.xaml`: `Button` and `AccentButton` brushes per state, `ButtonPadding`
  11,5,11,6, `FontWeight` Normal, `FocusVisualMargin` -3, the 83 ms `BrushTransition`.
- `controls/dev/CommonStyles/TextBox_themeresources.xaml`: `TextControl*` brushes, `TextControlElevationBorderBrush` and
  its focused twin, `TemporaryTextFillColorDisabled`.
- `controls/dev/CommonStyles/CheckBox_themeresources.xaml`: `CheckBoxCheckBackground{Fill,Stroke}*`,
  `CheckBoxCheckGlyphForeground*`, `CheckBoxSize` 20, `CheckBoxGlyphSize` 12.
- `controls/dev/CommonStyles/RadioButton_themeresources.xaml`: `RadioButtonOuterEllipse*`, `RadioButtonCheckGlyph*`, the
  20px ellipse, `RadioButtonCheckGlyphSize` 12 (14 on hover).
- `controls/dev/CommonStyles/ToggleSwitch_themeresources.xaml`: `ToggleSwitch{Fill,Stroke,KnobFill}{Off,On}*`, the 40×20
  `OuterBorder`, the 12px knob in a 20px cell.
- `controls/dev/CommonStyles/Slider_themeresources.xaml`: `SliderTrack*`, `SliderThumb*`, `SliderOuterThumbBackground`,
  the 18px thumb, the 12px inner dot and its `0.86` / `1.167` / `0.71` scales.
- `controls/dev/ProgressBar/ProgressBar_themeresources.xaml` and `ProgressBar.xaml`: `ProgressBarMinHeight` 3,
  `ProgressBarTrackHeight` 1, `ProgressBarCornerRadius` 1.5.
- `controls/dev/ComboBox/ComboBox_themeresources.xaml`: `ComboBox*` and `ComboBoxItem*` brushes, `ComboBoxPadding`
  12,5,0,7, `ComboBoxMinHeight` 32, `ComboBoxItemThemePadding` 11,5,11,7, the item margin 5,2,5,2 and corner 3, the 3×16
  pill, `ComboBoxDropdownContentMargin` 0,4.
- `controls/dev/CommonStyles/ToolTip_themeresources.xaml`: `ToolTipBorderPadding` 9,6,9,8, `ToolTipContentThemeFontSize`
  12, `ToolTipMaxWidth` 320, the in-app acrylic.
- `controls/dev/CommonStyles/ContentDialog_themeresources.xaml`: `ContentDialogBackground`, `ContentDialogTopOverlay`,
  `ContentDialogBorderBrush`, `ContentDialogSeparatorBorderBrush`, `ContentDialogSmokeFill`, `ContentDialogPadding` 24,
  `ContentDialogButtonSpacing` 8.
- `controls/dev/CommonStyles/ScrollBar_themeresources.xaml`: `ScrollBarThumbFill*`, `ScrollBarSize` 12,
  `ScrollBarThumbStrokeThickness` 6, `ScrollBarCornerRadius` 3.
- `controls/dev/Materials/Acrylic/AcrylicBrush_themeresources.xaml`: `AcrylicInAppFillColorDefaultBrush`, the same
  recipe as the menu flyout's, which is why the tooltip and the combobox list read `app-menu.css`'s surface tokens.
- `dxaml/xcp/dxaml/themes/generic.xaml`: `TextControlThemeMinHeight` 32, and the `HighContrast` system color behind each
  `SystemControl…Brush` a contrast-theme resource points at.

Two inputs aren't in the WinUI repo:

- **Elevation.** Learn's "Elevation in Windows"
  (`learn.microsoft.com/en-us/windows/apps/design/signature-experiences/layering`, last updated 2026-02-19, read
  2026-11-18 with Firecrawl): Dialog 128, Flyout 32, Tooltip 16, Control 2 (pressed 1). WinUI renders it with
  `ThemeShadow`, so the CSS is Fluent's own web token for each height from `microsoft/fluentui` commit
  `feab6ba63b55703d803f17092eab54c9dc82430b`, `packages/tokens/src/utils/shadows.ts`: `shadow16`
  (`0 0 2px ambient, 0 8px 16px key`) for the tooltip and `shadow64` (`0 0 8px ambient, 0 32px 64px key`), its largest,
  for the dialog. Ambient and key colors as `docs/notes/windows-port/menu-look-tokens.md` (light 0.12 / 0.14, dark 0.24
  / 0.28). Controls carry no shadow: their elevation is the border gradient.
- **Reference shots.** Learn's Button page (`learn.microsoft.com/en-us/windows/apps/develop/ui/controls/buttons`,
  `ms.date` 2026-10-13): `images/button.png` and `images/button-dialog-example.png`, compared against the Debug >
  Components catalog in the Windows look (2026-11-18). The ContentDialog's white content area over a grey command space
  with a separator, the 4px corners, and the button's darker bottom edge match.

## Derivations

- **Elevation borders.** `ControlElevationBorderBrush` is a 3px gradient: in Light it's flipped (`ScaleY -1`), so the
  bottom pixel is `ControlStrokeColorSecondary` and the rest `ControlStrokeColorDefault`; in Default (dark) it isn't, so
  the top pixel is the lighter one. A 1px border can't hold a gradient, so each edge gets its own token (`-border`,
  `-border-top`, `-border-bottom`). Text fields do the same with `TextControlElevationBorderBrush`'s strong bottom
  stroke, and the focused field's second accent pixel is an inset shadow (`TextControlBorderThemeThicknessFocused`
  1,1,1,2 without moving the text). The select trigger has no border in the house look, so its edge is inset shadows.
- **The accent.** WinUI fills with `SystemAccentColorDark1` (light) or `Light2` (dark) and writes fixed white or black
  on it. The backend sends one accent and no shade ramp (`system_look/platform/windows.rs` has no reader yet), so the
  Windows tokens fill with `--color-accent` and write `--color-accent-fg` (picked per accent for contrast). Hover and
  press are WinUI's own `AccentFillColorSecondary` / `Tertiary`: the fill at 90% and 80% opacity, as `color-mix()`. Once
  task 40's `UISettings` reader can send `AccentDark1` and `AccentLight2`, two tokens change and nothing else.
- **The checked radio and the slider thumb** are one element each in our DOM: the dot is the fill, and an inset shadow
  paints the ring around it (a 3px accent ring inside the 1px border leaves WinUI's 12px dot, 2px on hover leaves 14px;
  the slider's 2.84px / 1px / 3.74px rings leave its 10.3px / 14px / 8.5px dot).
- **The dialog surface.** `ContentDialog` stacks `LayerFillColorAlt` over `SolidBackgroundFillColorBase`; in dark that's
  `#0DFFFFFF` over `#202020`, composited to `#2b2b2b`. The command space (our footer) shows the base alone.
- **Contrast themes.** Each `HighContrast` resource maps to `Canvas`, `CanvasText`, `ButtonFace`, `ButtonText`,
  `Highlight`, `HighlightText`, or `GrayText`, directly or through generic.xaml's `SystemControl…Brush`. Buttons,
  checkboxes, switches, radios, the slider, the progress bar, the select, the combobox list, and the tooltip set
  `forced-color-adjust: var(--color-forced-adjust)` (`none` there) and paint the pairs; text fields leave Chromium's
  forcing on, since their inner text and affixes carry no contrast tokens.

## Proof that macOS and Linux didn't change

The house values are each primitive's old literal, moved into a token. Checked twice:

- **The whole catalog, computed.** A headless-Chromium probe (Playwright, against `pnpm dev`'s Vite) loaded Debug >
  Components with transitions off, recorded 41 computed properties of every element and its `::before` / `::after`
  (3,414 entries), statically and with `:hover`, `:focus-visible`, and `:active` forced on all 309 interactive elements,
  in light and dark, once on the commit before the tokens and once after. Result: 0 differences in each (2026-11-18).
  Keys that also differ between two runs of the same commit (spinners, the live transfer demo, 40 of them) were left
  out.
- **A snapshot of the house values** in `app-controls.test.ts`, so a later change to one is visible as a deliberate
  change to the macOS and Linux looks.

## What CSS can't carry, and what the tokens do instead

- **Disabled states without the dim.** The house look dims a disabled control with `opacity`, which stylelint's opacity
  allowlist won't take as a `var()`. So the dim is keyed on the look in the selector, inside `:where()` so the
  specificity stays, and Windows paints WinUI's disabled colors through `-disabled` twins that the control re-points its
  state tokens at. The select trigger keeps the dim on Windows too, close to WinUI's disabled ComboBox.
- **Glyph shapes.** The select's chevron is the house up-down stepper; WinUI's ComboBox shows a single down chevron
  (`E70D`). An icon is markup, not a token, so it stays until the select asks the look for its icon. The checkmark in a
  select list goes transparent on Windows (the pill marks the choice), but keeps its column.
- **Motion.** The switch knob's grow on hover (12 → 14px) and stretch on press, the slider dot's scale animation, and
  the radio dot's press shrink are WinUI animations; the tokens set the resting and hover sizes where one element can
  (radio, slider) and skip the switch's. Durations stay the house ones (task 62 owns motion).
- **Scrollbars.** WinUI's bar is an overlay that rests as a 2px indicator and expands to 6px on hover.
  `::-webkit-scrollbar` can't overlay or animate, so Windows gets the expanded bar (a 12px gutter, a 6px rounded thumb)
  in place of WebView2's 17px classic one; `--spacing-scrollbar-width` measures it as usual.
- **The font.** Segoe UI Variable and the type ramp are task 62's; the controls keep the system stack. The dialog title
  takes the ramp's 20px subtitle size.
- **Dialog layout.** `ContentDialog` stretches its buttons to equal widths and leaves 12px under the title; ours keep
  their natural widths and the title bar's 24px, since the footer also hosts leading content.
- **Not themed.** `ToggleGroup` (WinUI has no segmented control), `Chip`, and list selection (`ColumnList`'s cursor): no
  WinUI twin, so they keep the house look. Linux's neutral theme is a separate follow-up; until then Linux keeps the
  house look.
