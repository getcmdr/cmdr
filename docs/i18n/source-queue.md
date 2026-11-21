# Source queue

Translators log English-side problems here while translating: ambiguous or inconsistent English, a weak `@key`
description, a missing screenshot, or a rule every language should follow. One bullet per item, at most two lines: the
key(s), what's wrong, the suggested fix. The lead fixes the English, the description, or the shared docs (promoting a
rule into `translation-principles.md` or `translator-instructions.md`), then deletes the entry; git keeps the history.
Process feedback (a translator's homemade scripts and workarounds) goes under its own heading at the end.

## Open

- "AI take" (`aboutThis.verdict.*`, `commands.aboutThisStopVerdict.label`, `settings.aboutThis.aiVerdict.*`) has no
  concept, so each language coins its own word and nothing keeps the card, the setting, and the shortcut list aligned.
  Propose an `ai-take` concept. Also, `{kind}` / `{what}` in `aboutThis.realType.*` and `aboutThis.headline.*` render
  either Latin ("PDF") or Han ("磁碟映像檔"), so CJK spacing around them can't be right for both; a note saying which
  values are Latin would help. (zh-Hant)
- Screenshots: `mtp-browse.png` doesn't show the `mtp.readiness.connecting` row, and the AI suggestions dialog keys ship
  without any (the pills and fact lines need them most). Needs a `pnpm i18n:shots` run with the Mac left alone. (all)
- Counted nouns in the RAW `errors.*` family can't take an ICU plural, so Slavic locales still need a colon form:
  `errors.write.trashRefused.message.*`, `errors.write.deviceDisconnected.sided.*`,
  `errors.eject.unmountRefusedByProcesses`. Either word around the count, or give the raw path a plural. (uk)
- `archive` concept: the zip-noun keys (`fileExplorer.list.rowKind.archive`, `suggestedOps.facts.archive`) now leave
  `archive-chat`, but `archive`'s own `notMatch` drops any value that's exactly "Archive", so they match neither. Needs
  a key-scoped `match`. (all)
- `menu.window.zoom` and `menu.view.zoom` are both "Zoom" in English, so cs, fr, and uk keep a `zoom` exception; split
  the English or accept it. Needs David: it renames a menu item. (uk)
- The `remove` / `original` concepts match `operationLog.tree.statusRemoved`, `skipReason.locked`, and
  `skipReason.restoreTargetOccupied`, which their own descriptions exclude. (cs, de, es, fr, hu, nl, pt, ru, sv, uk, vi,
  zh, zh-Hant)
- `operationLog.dialog.selectedCount` drives its plural with the TOTAL, but "selected" agrees with the checked count
  (`3 sur 12 sélectionnés`), so fr falls back to a colon form. Pass the checked count as the plural driver: a code
  change plus a new key, since translations agree with the old driver. Likewise `operationLog.tree.backfillProgress` has
  no plural driver at all for "Read {doneText} … items". (cs, es, fr, pt, sv)
- `search.coverage.line.uncovered.*`: `{drive}` is either a name or the generic phrase from
  `search.coverage.unnamedDrive`, so case-marking languages read stiffly ("ezt: ez a meghajtó"), and in `.unavailable`
  it actually holds a SERVER name. Give the generic case its own variant key, or a server param of its own. (hu, ru)
- `look-inside` is defined as the AI reading a file, yet `search.coverage.line.denied` / `.declined` use it for a walk
  entering folders: widen or exclude. (cs, ru)
- "system" (the OS, `search.coverage.line.offLimits`) has no concept yet, and every locale coined its own word; add one
  sourced from Apple's localized macOS. (sv, zh, zh-Hant, uk)
- vi: `search.coverage.deniedFullDiskAccess` and `search.coverage.setUpFullDiskAccess` say `quyền truy cập toàn bộ đĩa`,
  but Apple's pane is `Quyền truy cập đầy đủ vào ổ đĩa` (vi `style.md`). Re-translate both. (vi)
- "Couldn't X" sentences naming Cmdr as the subject (`search.coverage.line.failedRoots`) collide with a style guide's
  impersonal pattern ("X ließ sich nicht öffnen") because `dont-translate` demands "Cmdr". Decide whether an actor-only
  "Cmdr" may drop in translation. Needs David. (de)
- `pnpm i18n:brief`'s regional-overlay section names vocabulary forks only; `es-419` also forks the compound perfect to
  the preterite ("no pudo"), which cost a failed check. Name tense forks there. (es)
- Briefs for brand-new keys: run `sync-locale-keys.ts` before `pnpm i18n:brief`, or the brief describes skeletons the
  catalog doesn't hold yet. (cs, de, es, fr, nl, pt, sv)
- `ask` concept matched "Your settings ask for" (`settings.ai.provider.localUnavailable`), the request sense; added
  `settings ask for` to its `notMatch`. A broader "ask for" exclusion needs a call, since prose "asks for your password"
  sits in the concept. (all)
- `settings.fileExplorer.typeToJump.mode.description`: the `other` branch names the PC's Backspace key, but no concept
  covers it (`delete-key` is the Mac ⌫ key), so each locale records its PC key name ad hoc (es put `Retroceso` in
  `delete-key`'s `other` form). Propose a `backspace-key` concept. (es)
- `recent` matches "recently closed tabs" (`settings.fileExplorer.tabs.closedTabHistorySize.description*`), which vi
  renders as "just closed" (`vừa đóng`), as its `commands.handler.noRecentlyClosedTabs` already does under an exception.
  Only vi deviates, and the concept's sense names recently closed tabs, so it stays a vi exception unless another locale
  does the same. (vi)
- The pile has no Windows shell label "This PC" (the Microsoft TBX lacks it), so the `windows` branches naming it rest
  on judgment and stay `tentative`. Source it from a localized Windows, or a Microsoft support page per language. (es,
  hu, vi)
- `errors.listing.authRequiredEneedauth.suggestion`: the `other` branch serves macOS AND Linux but says "in Finder", so
  a Linux user is sent to Finder. Give it a `macos` branch and a generic `other` ("in your file manager"). (hu)
- Windows shell labels the pile can't confirm: "Associated Handles" (Resource Monitor), "Empty Recycle Bin", the
  "Read-only" attribute checkbox, "Tools > Check", "user folder", and Settings > System > "Storage" have no exact TBX
  hit, so every locale builds them from parts. Source them once from a localized Windows per language. (hu, de, cs, uk)
- `quoted-labels` pairs the Windows branch's “Read-only” (File Explorer's attribute checkbox) in
  `errors.write.permissionDenied.suggestion.delete` and `errors.write.fileLocked.suggestion` with Cmdr's own
  `errors.listing.readOnly.title`, so quoting Windows' real label (cs „Jen pro čtení“) fails the check against Cmdr's
  „Jen ke čtení“. Give the check a per-key opt-out, or skip pairs where the quote names another OS's control. (cs, uk:
  quoted Cmdr's «Тільки читання» to pass, where Windows likely says «Лише для читання»)
- `allow` matches "wasn’t allowed to" / "isn’t allowed to" (a file-system refusal, not granting consent) in
  `errors.write.trashRefused.message.notPermitted` and `…folderNotWritable`; hu carries two exceptions. Propose a
  concept `notMatch` for both phrases. (hu, de: `…noPermissionErrno.explanation`, `…permissionDenied.explanation` too)
- `select` on Windows means "choose a command" (`select **Properties**`), which collides with the file-selection
  `select` concept in every locale that splits the two; hu added a `windows` form. Consider a `notMatch` for "select **"
  or a `choose` match in Windows branches. (hu)
- No concept covers "system monitor app", "disk tool", "network adapter", "Resource Monitor", or "Start" (the Windows
  menu), so each locale coins them unrecorded. (hu, de)
- English senses the Windows and Linux branches add to macOS-shaped concepts: `ai-tool` hits "**Tools > Check**" (a
  Properties tab), `desktop` hits the Windows desktop and "your desktop’s disk tool" (a Linux desktop environment),
  `tab` hits Resource Monitor's "CPU tab", and `scan` hits "scan it for problems". Propose `notMatch`es or concepts.
  (de, uk: `i18n:apply-terms --concepts` refuses the `notMatch` because it would stale other locales' exceptions (cs,
  nl, ru, sv), so uk carries exceptions too; a lead-side `--drop-stale-exceptions` pass would settle it)
- `errors.listing.notEnoughMemory.suggestion`: the note keeps "Ctrl+Shift+Esc" as-is, but a German keyboard reads
  Strg+Umschalt+Esc (French and others differ too). Say whether key names may localize in a Windows branch. (de)
- The disk-check keys (`errors.listing.diskReadProblem.suggestion` and three siblings) call "GNOME Disks" a product
  name, but GNOME localizes the app's name (de "Laufwerke"). Say whether the Linux branch keeps the English name. (de)
- A new ruling for `file-explorer` flags `fileExplorer.pane.fileExplorerAriaLabel` ("File explorer", Cmdr's own pane
  area) in every locale; add that key to the concept's `notMatch`. (de, uk)
- `app-closes` matched "Cmdr closed the connection" (the four `*negotiationInvalid` error keys), so any locale with a
  ruling drifted there; added `closed the connection` to its `notMatch`. (hu)
- English-sense mismatches in per-OS branches: `path` hits "your PATH" (the environment variable,
  `settings.fileOperations.adbBinaryPath.description`), `unplug` hits "Nothing gets disconnected"
  (`fileExplorer.network.browser.removeHostConfirm`), and `system-settings` hits the generic "your system settings"
  (`settings.appearance.themeColorSwatchAria`). hu carries an exception and an `accept`; propose `notMatch`es. (hu)
- `settings.appearance.appColorHintPrefix` has one `other` branch for Windows and Linux, but its link
  (`appColorHintLink`) is a Settings path on Windows and a noun phrase on Linux, so the prefix can't end in the colon a
  path wants. Give the prefix a `windows` branch, or make it one sentence with a link tag. (hu)
- "System disk" (Linux) and "system drive" (Windows) have no concept: they ride on `startup-disk`, and no Linux catalog
  in the pile names a system disk. Also unconfirmed Windows labels: "Settings > Personalization > Colors", "Settings >
  Apps", and "Windows display language". (hu)
- `fileExplorer.network.browser.refreshHint`: Windows' input-neutral "select here" has no natural rendering where the
  target is the hint text itself; say what "here" is ("select this hint") in the description. (hu)
- `file` matches the app name "File Explorer", so a locale whose Windows name has no file word (sv `Utforskaren`) drifts
  on every Windows branch naming it; sv carries six exceptions. Add "File Explorer" to `file`'s `notMatch`. (sv, uk: a
  `windows` form on `file` accepting «Провідник»)
- `software`'s sense covers "the hardware they run on", so a first ruling (sv `programvara`) drifts on "hardware-level"
  keys (`errors.listing.deviceProblem.explanation`, `…diskReadProblem.explanation`) and on
  `settings.network.smbConcurrency.description`; split hardware into its own concept. (sv)
- `accent`'s sense is a diacritic, yet it matches "accent color" in `settings.appearance.appColor.description`,
  `.themeColorSwatchAria`, and `.appColorHintPrefix`; add "accent color" to its `notMatch` or give the Windows accent
  color its own concept. Likewise `prompt` (AI text) catches "system prompt" (a dialog asking the user), which sv words
  as `fråga`. (sv, uk)
- `aboutThis.what.file` ("File", the item kind) shares its English with `menu.bar.file`, which Apple localizes as a menu
  name (sv `Arkiv`, de `Ablage`), so `term-consistency` fails for every such locale. Needs a reasoned `reviewed` entry
  (David's consent) or an `agreesWith`-style escape for menu-bar names. (sv, de: same `reviewed` entry added)
- `aboutThis.standardFolder.*`: the descriptions say "use the name Finder shows", but the pile has no Finder folder
  names; they live in macOS's `SystemFolderLocalizations/<lang>.lproj` (de Library stays `Library`, Users is
  `Benutzer:innen`). Add that file to the pile and to `how-to-mine.md`. (de)
- `aboutThis.ownerPlace.*`: the Library subfolder names are kept as on disk; give them a shared same-as-source rule (and
  `notMatch` for `group`/`log`/`save`, which sv added key-scoped) so 12 more locales don't each justify nine keys. (sv)
- `aboutThis.signature.ticketPresent` / `.ticketAbsent`: "notarization ticket" has no hit in any pile; name Apple's
  localized term in the description, or say a plain coinage is fine. (sv)
- `model-library` (Ollama's catalog) matches the macOS Library folder in `aboutThis.label.appData`,
  `.standardFolder.homeLibrary`, `.removal.removesApp*`, and `ai.cloudConsent.aboutThis.item.appData`; give the Library
  folder its own concept (Finder's name) or add these to `notMatch`. (fr; hu added `your library` and `library folder`
  to its `notMatch` as the stopgap, plus key-scoped `notMatch` for `cache` and `preferences` on `ownerPlace.*`)
- `aboutThis.cmdrOperation.done`: the `{who}` select (you / Ask Cmdr / an AI client) is followed by a separate `{kind}`
  verb select, so a language that conjugates by person (hu `másoltad` vs `másolta`) can't agree; hu fell back to a label
  frame (`Általad indított Cmdr-művelet: ide másolás`). Allow `{kind}` nested inside each `{who}` branch, or say so in
  the description. (hu; es nested `{kind}` inside each `{who}` branch, which `i18n:apply` and every check accept, so the
  description only needs to say it's allowed)
- `aboutThis.headline.*`, `.realType.*`, `.owner.standardFolder`: `{what}` / `{kind}` / `{folder}` arrive with unknown
  gender before an English participle ("copied here", "checked against"), so gendered languages must use nouns. Say so
  in the descriptions, or pass a gender select. Same for `importance.because`: the reasons start with articles. (fr)
- `aboutThis.headline.lastOpened` and `aboutThis.lastOpened.known` put a bare `<date></date>` ("last opened <date>"),
  while every sibling says "on <date>". Say in the description whether the date can be relative ("yesterday"), so a
  language knows whether it may add its "on" (nl `op`). nl left the preposition out to be safe. (nl)
- `write` (data landing on a drive) matches the AI composing text in `aboutThis.verdict.writing`,
  `.verdict.problem.noAnswer`, and `settings.aboutThis.aiVerdict.description`; add the AI sense to `notMatch` or give it
  its own concept. cs carries three key exceptions. (cs)
- `commands.fileTellMeAbout.label` and `aboutThis.continueInAskCmdr.draft` are the user speaking TO Cmdr, which no
  formality ruling covers (de took du, fr vous). Decide per locale whether the app's own address register applies; cs
  followed its vy-plural (`Povězte mi o tom`). (cs)
- `quoted-labels` pairs Windows' own "Read-only" checkbox (quoted in `errors.write.permissionDenied.suggestion.delete`
  and `errors.write.fileLocked.suggestion`) with Cmdr's unrelated `errors.listing.readOnly.title`, so cs fails on
  correct text (Windows says `Jen pro čtení`, Cmdr's title `Jen ke čtení`). Exclude OS-quoted labels in platform
  branches, or key-scope the pairing. (cs)
- The `location` concept matches a photo's GPS place in `aboutThis.verdict.given.cameraDetails` and
  `ai.cloudConsent.aboutThis.lookInside` (as it already did in `ai.cloudConsent.askCmdr.item.contents`), so every locale
  that splits a photo's place from a file-system location carries per-key exceptions. Add a `notMatch` for the photo
  sense ("photo’s … location", "without its location"). (zh)
- The `desktop` concept matches the Linux desktop environment ("your desktop’s disk tool", "your desktop usually
  mounts", "Your desktop may also") in four `errors.listing.*.suggestion` keys, `mtp.ptpcameradDialog.explanation`, and
  `onboarding.stepOptional.mtp.desc`, so uk and vi each carry six exceptions. Add those as `notMatch` and drop them.
  (vi)
- `fileOperations.validation.disallowedCharacter` / `endsWithSpaceOrPeriod` say "Filenames", while their
  `multiRename.reason.disallowedCharacter` sibling says "Filenames"; pick one spelling. (all)
- `updates.toast.readyDetail`'s `other` branch serves Windows and Linux, but most locales' `app-closes` ruling is
  Windows-only (Linux says "quit"), so Linux reads the Windows verb. Split a `windows` branch if Linux should differ.
  (all)
- `askCmdr.renameReview.notPortableBadge` is "(Windows)", identical in every locale, so each one needs a
  `sameAsSourceJustification`; the brand-only check could treat a parenthesized brand as identical by design. (cs, ru,
  uk, hu)
- KDE branches: the descriptions say KDE keeps "KWallet" / "KWalletManager" as names in every language, but KDE zh-CN
  ships `KWalletManager 密码库管理器` and zh-TW `KDE 錢包管理員` as the launcher name; we kept the Latin names. Check
  whether the descriptions should allow the localized launcher name. (zh, zh-Hant)
- The `ai.secretError.body` / `.removeBody` `other` branch still says "(Passwords / Keyrings app)", GNOME's app name,
  now that GNOME has its own branch; the `other` branch could name the keyring desktop-neutrally. (all)
- GNOME vi's own catalog misspells Keyboard as `Bàm phím` (gnome-control-center vi.po); vi's
  `shortcuts.conflict.takenByLinuxDesktops` writes `Bàn phím`. Worth an upstream GNOME fix. (vi)
- `ai-tool` matches "the adb tool" / "another tool" (`settings.fileOperations.adbEnabled.summary`), and `note` matches
  the verb "notes" (`settings.securityLog.enabled.description`); each locale added an exception. Add `notMatch`. (all)
- `licensing.section.includedAi`: "your own key" reads as the license key on the License page; every locale wrote "API
  key". Say "your own API key" in English. (de, hu, nl, vi)
- `settings.network.requireSmbEncryption.summary`: "guests need to sign in" contradicts itself once translated ("a guest
  who signs in isn't a guest"). Try "guest connections won't work; sign in instead". (es, fr, hu, nl, zh-Hant)
- `settings.adb.pathPlaceholder`: "Found automatically" reads as a past result on an empty field; "Finds it
  automatically" says what an empty field does. (de, nl, zh)
- Bare "It" with no on-screen antecedent: `settings.securityLog.enabled.description`,
  `settings.analytics.enabled.description`, `settings.askCmdr.proactive.description`,
  `settings.advanced.logLlmCalls.description` ("Cmdr writes them"). Name the subject in English. (de, fr, nl, sv)
- `settings.askCmdr.provider.usesProvider`: `{provider}` as a verb object breaks case agreement when it's "Local LLM";
  locales used a colon slot. Rephrase English or hint it in the description. (de, ru, uk)
- `settings.ai.reasoningEfforts.summary`: the `@key` says to echo no label word, yet "More thinking" repeats "Thinking
  level". Fix one or the other. (cs, ru, vi)
- `settings.askCmdr.interactiveModel.placeholder` drifts from "Same as Cmdr's AI" in hu (and cs lacks a no-break space),
  so the quoted `sameAs` label differs from it; retranslate the placeholder with `sameAs`. (cs, hu)
- `settings.control.moreAbout`'s `@key` asks for quote marks, which hu's `learn-more` ruling forbids. (hu)
- `settings.mtpEnabled.*`, `settings.fileOperations.adbEnabled.summary`: the Android on-phone labels ("USB Preferences",
  "File transfer") have no source in the pile; add AOSP Settings catalogs. "“Android Auto” works too" is unclear. (all)
- `settings.appearance.dateTimeFormat.optDesc.short`: the month-first sample `01/25` reads backwards in day-first
  locales; say whether it should follow the locale's format. (fr, pt)
- `settings.search.autoApply.description`: "AI searches wait" lost its object ("for Enter"). (zh)
- `settings.askCmdr.proactive.summary`'s `@key` names the Suggested ops dialog's wording without a key; locales matched
  `settings.suggestedOps.suppressApprovalNotice.description`. Name the key. (ru, sv)
- `settings.network.timeoutUnit`: a unit after a free number can't pluralize; use an abbreviation or an ICU plural. (sv)
- `settings.behavior.notifications.keepUntilClosed.description` is a fragment ("Instead of …") that needs a pronoun in
  several languages; a full sentence would carry better. (ru, uk)
- vi `settings.behavior.fileSystemWatching.lowDiskSpaceNotifications.label` uses `Thiếu`, which its `free-space` ruling
  avoids; retranslate. Czech `quoted-labels` warns on Windows' own „Jen pro čtení“ in two `errors.write.*` keys; the
  check needs a carve-out for quoted OS labels. (vi, cs)
- zh and zh-Hant now quote KDE's localized wallet manager launcher name (zh `KWalletManager 密码库管理器`, zh-Hant
  `KDE 錢包管理員`, from `kwalletmanager._desktop_.po`) in `fileExplorer.restic.forgetPassword.keychainKeptIt`, which
  answers the older KWalletManager entry above for those two; the descriptions still say "KWalletManager (an app name,
  kept as is)". Let them allow the catalog's launcher name. (zh, zh-Hant)
- Overlay forks whose English gained per-OS branches kept those branches in English, overriding the base language's
  translation: es-419 had English `windows` / `other` text in `onboarding.stepBeta.crashReportsNote`,
  `.crashReportsNoteAlone`, `.signup.unreachable`, and `mtp.connectedToast.body` (fixed now). Coverage didn't flag them
  (stale only said the fork was stale); a check that flags an overlay fork with a non-empty branch byte-identical to
  English would. (es-419)
- GNOME's lowercase pointers ("in preferences" in `askCmdr.error.*`, `askCmdr.wake.needsApiKey`,
  `mtp.connectedToast.body`) read as an everyday word where GNOME's term is one (vi `tùy thích`, "as you like"); KDE now
  names the window "Configure Cmdr" there. Proposed rule: a pointer naming the settings window is capitalized as a name
  on GNOME too. Rewords the GNOME branch only. (es, pt, vi, zh, zh-Hant)
- `settings.indexing.enabled.label` and `settings.section.driveIndexing` ("Drive indexing") are plain values, quoted by
  the `windows` branches of `fileExplorer.navigation.driveIndex.*` and `search.coverage.toast.indexingOff`. Windows'
  French word for a drive is `lecteur` (the `drive` ruling's Windows form), so the label reads `Indexation du disque` on
  Windows while the prose around it now says `lecteur`. A per-OS label would let the two agree. (fr)
- Six keys whose `windows` branch names the settings window carry no `identicalBranches` note and weren't stale, so a
  translator finds them only by grepping: `menu.app.settings`, `fileExplorer.functionKeyBar.hiddenToast`,
  `fileExplorer.quickFilter.intro.openSettings`, `onboarding.stepBeta.signup.unreachable`, `whatsNew.optOutToast`,
  `ai.cloudConsent.logsNote` ("Advanced settings"). es and pt changed them to Microsoft's word; other locales where it
  differs (fr, de, hu, ru) likely need the same. (es, pt)
- The seeded `windows` branches copied the macOS text whole, so they also carried macOS words beyond "Settings" (pt
  `disco` for drive, `iniciar a sessão` for sign in), which the termbase caught. And the plain Settings label
  `settings.indexing.enabled.label` ("Drive indexing", pt `Indexação de disco`) then pins `disco` in four Windows
  pointers that quote it: a per-OS value there would let Windows say `unidade`. (pt)
- `errors.write.linksNotSupported.suggestion`'s description says don't translate "Mac OS Extended", but Disk Utility
  localizes it (cs `Mac OS rozšířený`, es `Mac OS Plus`, fr `Mac OS étendu`, zh `Mac OS扩展`; the new `mac-os-extended`
  concept has every ruling). Fix the description to say "Apple's localized name". (cs, es, fr, hu, nl, pt, zh, zh-Hant)
- `errors.write.linksNotSupported.message`: "didn't copy the link to {path}" also reads as "the link that points to
  {path}". "didn't put the link at {path}" would be unambiguous. Its "links" in the title and suggestion are symlinks,
  which the `link` concept caught; now key-scoped `notMatch` entries. (all)
- `menu.view.showMenuBar` and `settings.appearance.menuBar.label` share English, so `term-consistency` forces one form,
  against the house split `menu.view.showHiddenFiles` has (nl `Toon` vs `tonen`, ru `Показать` vs `Показывать`). Both
  now use the settings form, which KDE's own `Show Menubar` item matches; split them with a reasoned allowlist entry if
  David prefers the menu register. (nl, ru)
- `errors.write.cantTellIfSameFile.message` is a plain value saying "This drive", but cs, fr, and pt rule a Windows form
  for drive (`jednotka`, `lecteur`, `unidade`), so Windows readers get the macOS word. Make it per-OS, or accept the
  macOS word in plain `errors.write.*` values as a family rule. (cs, fr, pt)

## Process feedback

Translators log here every script they wrote and every workaround they needed (the report's Process feedback section,
`translator-instructions.md`): one bullet per item, the tool or step that was missing, and the locale. The lead reads it
after each wave. Two locales logging the same kind of script is a missing tool: build it or fix the brief, then delete
the entries (`docs/guides/multi-agent-refactors.md` § Fan-out sweeps).

- The worktree guard refused a plain shell `for` loop over `node scripts/i18n-values.ts --lang $l` (no git in it), so
  per-locale reads need `--lang all` or one call per locale. (all locales, per-OS settings-window batch)
- The same guard refused a `for` loop over `i18n:pile` and a `W=…` variable before `i18n:check-locale`; a `--lang all`
  (or several `--lang`) on `i18n:pile` and `i18n:check-locale` would save 13 calls each. Recording one new concept's
  ruling in 13 locales also took a one-off `node -e` that wrote 13 patch files: `i18n:apply-terms` could take one
  multi-locale patch. (all, menu-bar batch)
- `--lang all` in `i18n:values` and `i18n:brief` leaves out the `es-419` overlay, so its forks needed a separate
  `i18n:values --lang es-419` read and a hand-made values file under `_ignored/i18n-work/es-419/manual/` (the brief
  creates no run directory for it). A `--lang all` that includes overlay forks would save both. (es-419)
- `i18n-terms` reports a stale allowlist entry ("no longer diverges; drop it") but never rewrites
  `i18n-term-consistency-allowlist.json`, unlike the shrink-wrapping allowlists, so the entry had to be removed by hand.
  (es, pt)
- The brief for 88 per-OS keys was 445 KB (about 110k tokens): too big for one read, and the Keys section alone needed
  three. Each key repeats its English three times (the value, the word diff, and the English branches inside "now") and
  an identical boilerplate bullet ("Per-OS value"). Print it once per brief, drop the diff when the only change is the
  per-OS wrap, and show only the branches to translate plus the macOS one. (cs batch 1: 88 keys, 133k tokens, read in
  five chunks even with `allow_large`; hu, de: read in nine chunks, no script; hu batch 3: 122 keys, 385 KB, about 161k
  tokens, read in four Keys chunks plus one `grep -A3` of the Terms section, no script; es batch 2: 105 keys, 2,397
  lines, about 145k tokens, read in seven chunks, no script)
- Wrote a one-off `grep -v` / `gawk` filter to strip those lines from the brief so the keys fit in context. A
  `--compact` brief would replace it. (hu)
- The worktree guard refuses any compound shell command it can't verify (a heredoc, a `grep | sort | uniq` chain over
  the catalog), so patch files went through the Write tool and a catalog lookup was retried as a plain `grep`. Say so in
  the instructions, and offer a catalog search (`pnpm i18n:grep --lang hu <regex>`) for "how did we ship this word".
  (hu)
- Per-OS drift surfaced only at the check: 34 findings, nearly all from five causes (Windows `Properties`, `select`,
  `Storage`, `Settings`, plus three English-sense mismatches). The brief could list, per ruled concept, "no `windows`
  form yet" for concepts that hit a Windows branch, so the translator patches `terms.json` before applying. (hu, de: 17
  findings from six causes)
- `pnpm i18n:apply-terms --help` exits with "Unknown option", and the brief doesn't show the `avoid` entry shape
  (`{ form, why }`), so the patch needed a look at a neighbor ruling first. Add `--help` with the field shapes. (de)
- A "no ruling" concept that only a Windows or Linux branch uses (`file-explorer`, `task-manager`, `user-folder`) never
  fails the check, so skipping its ruling goes unnoticed; the check could warn when a translated branch hits one. (de)
- `decisions.md` sat exactly at its byte budget, so one new entry needed a hand-edit of
  `apps/desktop/scripts/i18n-termbase-baseline.json`; `i18n:apply-terms` could take a `--bump-decisions` flag. (hu)
- The brief's "Translation memory" section (66 KB) went unread: for a per-OS batch the `macos` branch is the memory.
  Skip it when every key is a per-OS restale. (hu)
- A 30-key per-OS brief (1,144 lines, about 69k tokens) is over the Read tool's 25k-token cap, so `limit: 1200` alone is
  refused and the read needs `allow_large: true`; say so in the lead's prompt, or trim the brief. (sv) A 105-key brief
  (2,383 lines, about 141k tokens) doesn't fit even with `allow_large`, so the terms section took an `awk | grep`
  filter. (fr) An 88-key brief (2,025 lines, about 134k tokens) took six paged reads even with `allow_large`. (nl) uk
  batch 1 (2,034 lines, 131k tokens): two paged reads of the Keys plus a `sed | grep` of the Terms section's rulings.
  (uk)
- A windows/other termbase form replaces the base ruling's `accept` list instead of inheriting it, so each OS form needs
  its own `accept` copied in; the first nl check flagged 42 keys for that alone. Inherit the base accepts. (nl)
- The worktree guard refused `apply-terms && apply && check-locale` chained with a shell variable ("names git in a form
  too complex"); split into three calls. (nl, uk: a heredoc plus two `apply-terms` calls, redone as Writes and separate
  calls; sv: same)
- A 353-key new-feature brief was 356 KB (3,751 lines, about 90k tokens): Keys took three paged reads, Terms one
  `sed | grep` for the rulings, and the 1,500-line Translation memory only a `grep`. Ship a keys-only first read for
  new-key batches. One inline `node -e` edit of my own `values.json` to fix eight values after the check. (sv; de: same
  reads, no script)
- The worktree guard refused a read-only `for` loop of `jq` lookups over the catalog and a two-command `jq` call ("names
  git"/"too complex"); retried as one plain `grep`. A `pnpm i18n:grep` would save it. (de)
- `apply-terms` refused a `learn-more` exception as stale because another translator's concurrent `notMatch` had already
  excluded the key; `pnpm i18n:term --key` showed it. (de)
- Read Finder's folder names straight from the live macOS `SystemFolderLocalizations` (plutil, read-only), since the
  pile lacks them; widened `de/mechanics.json`'s gender-colon carve-out to the quoted folder name „Benutzer:innen“. (de)
- The worktree guard refused a `python3` heredoc that patched my own `values.json` and a call using a shell variable for
  the run directory ("names git" / "too complex"); redone as 12 `Edit` calls and full-path single commands. No script
  written. Confirmed Finder folder names with a read-only `plutil` on `SystemFolderLocalizations` too. (fr)
- One `python3` heredoc (five exact-string replacements in my own `values.json` after the termbase check) ran; a later
  chain of two `apply-terms` calls was refused by the worktree guard and split. A `plutil` read of
  `SystemFolderLocalizations` showed Finder's hu Desktop folder is `Íróasztal`, not the ruled `Asztal`: the pile should
  carry that file. (hu)
- `--dry-run` of `i18n:apply` passes values the termbase then flags; a `--check-terms` option on the dry run would catch
  drift before writing. The termbase also flags a git tag as the Finder `tag` concept in every language whose two words
  differ; a shared `git-tag` match would save per-locale exceptions. (pt)
- Reworded four termbase-flagged values with one `sed` over my own `values.json` (no catalog touched); same wish as pt:
  a term check on `i18n:apply --dry-run` would have caught them before writing. (nl)
- For a "no ruling" term the brief shows only the pile, not how the catalog already words it: es coined `equipo de TI`
  and `en el disco` (pile forms), and the termbase check then flagged 13 shipped keys that say `equipo de informática` /
  `en disco`. Show the catalog's current words for each "no ruling" concept beside the pile. Two `sed -i` edits on my
  own values files (three phrase swaps, a subjunctive fix in the es-419 forks), no catalog touched. (es)
- The worktree guard refused `pnpm i18n:pile` because one `--term` value was the English word "alias" (it reads the word
  as a shell alias); dropping that term worked. A pile lookup argument should never trip the guard. One `sed -i` on my
  own `values.json` for a wording fix, no catalog touched. (uk)
- `apply-terms --concepts` refused the `desktop` `notMatch` (Open above) because uk's exceptions for those keys would go
  stale, and its hint `--drop-stale-exceptions` would edit another locale's file, so vi took six exceptions instead. A
  new ruling for a no-ruling term (`on-disk-size`, `importance`, `desktop`) also flagged 15 shipped keys only at the
  check; `apply-terms` could report a new ruling's catalog drift as it writes (same root as es above). No script; one
  read-only `plutil` + `jq` over CoreServicesUIAgent's loctables hunting Apple's vi "notarization" (none found). (vi)
- A 7-key batch for 13 locales: the worktree guard refused a `for` loop of `pnpm i18n:brief` calls, so each brief was
  its own call; a `--lang all --out-dir` mode writing one brief per locale would save it. The termbase check also failed
  on `decisions.md` budgets (de, es, nl, pt, zh: 2–10 bytes over) that this batch didn't touch. (Windows port batch)
- cs, ru, uk, hu batch: no scripts written. The 16 new keys needed `sync-locale-keys.ts <tag>` before `i18n:apply` would
  take them (the coverage check reports them as "missing", the brief doesn't say to sync). The term-consistency
  allowlist had to be hand-edited (no tool writes it). `i18n:apply-terms` replaces a ruling's `note` whole, so adding a
  sentence silently dropped the uk `keychain` note until a second patch restored it; an `appendNote` (or a warning on a
  replaced note) would prevent that. The worktree guard refused commands whose path contains `projects-git` when they
  also used a subshell or `cd` chain. (cs, ru, uk, hu)
- `pnpm i18n:brief --missing` only finds keys ABSENT from the catalog, so after `sync-locale-keys.ts` adds the English
  skeletons it selects nothing, and a second locale's brief needed the first one's `keys.txt` passed by hand
  (`--keys-file`). `i18n:apply` also refuses absent keys until a sync, so the brief can't come first. A `--untranslated`
  selector (coverage's definition) would cover both. (de, nl, sv, fr)
- A `windows` / `other` form on a ruling (`quit` in nl) also judges every PLAIN value, so a Linux-only verb made 20
  plain `quit` keys drift; a single key's exception was the only way out. Scoping per-OS forms to block branches would
  let them carry Linux words. (nl)
- The GNOME and KDE branches need catalogs the pile lacks: Seahorse ("Passwords and Keys", keyring),
  gnome-control-center (Settings > Keyboard > View and Customize Shortcuts), and KDE's systemsettings, kcm_keyboard,
  kxmlgui, kwalletmanager. Fetched them read-only into a scratch dir with two small `curl` scripts (l10n.gnome.org,
  websvn.kde.org `?view=co`); the pile should carry them. Also built the batch's `keys.txt` by hand from the stale +
  coverage output: a brief selector for "everything stale or missing for this locale" would save that. One read-only
  `node` lookup listed per-OS branches still in English. The guard refused `for` loops and any command whose text
  contained a URL or `$?`. (es, pt, vi, zh, zh-Hant)
- Second translator to write the same catalog-fetch `curl` script (seahorse, gnome-control-center, kconfigwidgets,
  kwalletmanager, systemsettings, kcm_keys/kcm_keyboard metadata), and it found two wrong GNOME paths and two wrong
  wallet/keyring words that the pile couldn't have caught: the pile should carry these catalogs, or `i18n:pile` should
  fetch them. Working URLs: `l10n.gnome.org/POT/<module>.main/<module>.main.<tag>.po` and
  `websvn.kde.org/trunk/l10n-kf6/<tag>/messages/<repo>/<catalog>.po?view=co`; KCM names live in the `kcm_*.json`
  metadata on invent.kde.org, not in a `.po`. KDE's `Configure %1…` lives in `kconfigwidgets6.po`, not `kxmlgui6.po`
  (obsolete there). A `--lang` list on `i18n:brief` (one brief per language in one call) would have avoided 13 separate
  commands, since the guard refuses a loop. (de, nl, sv, fr)
- `i18n:apply-terms` has no dry run: a first ruling for a "no ruling" term flags drift in keys outside the batch,
  costing one to three patch rounds. Preview catalog-wide drift before writing. `sources` must be a string; say so in
  the brief. (fr, hu, nl, pt, sv, vi, zh)
- `i18n:apply` prints "add sameAsSourceJustification" for keys whose catalog `@key` already has one, and `--dry-run`
  after a real apply reports dropped forks as failures. (es, nl, sv, zh-Hant)
- The brief needed `node scripts/sync-locale-keys.ts <tag>` first (38 keys missing from the catalog); the brief could
  run it. (de, fr, nl, vi)
- `decisions.md` budgets sit at the files' current size, so a parallel translator can't record a decision without
  editing the shared `i18n-termbase-baseline.json`; vi and zh-Hant reverted theirs and kept the evidence in `terms.json`
  notes. (vi, zh-Hant)
- A ruling's `windows` / `other` form REPLACES the base `accept` for those branches instead of merging it, so adding one
  to `settings` raised drift on three unrelated keys until the base accept was repeated in each form. Document it, or
  inherit. (vi)
- A deliberately empty English branch (`mtp.connectedToast.body`'s `windows {}`) counts as "still English" and needs a
  `sameAsSourceJustification` in every locale; an empty source branch could be exempt. (es, pt, vi, zh, zh-Hant)
- A ruling form scoped to the KDE branch (`linuxKde` beside `windows` / `other`) would replace the KDE `configure` words
  each locale carries as an `accept` (46–56 termbase warnings cleared that way). (de, nl, sv, fr)
- cs, ru, uk, hu settings-window batch: no catalog or termbase script. Each `values.json` was a `jq` one-liner turning
  `keys.txt` into `{ "keep": true }` entries, merged with a hand-written file of the branches to change; a
  `pnpm i18n:apply --keep-rest` (keep every batch key the file doesn't name) would replace it. `i18n:values` has no way
  to show only the branches still in English, so finding the `linuxKde` gaps took a `grep "= English"` over its output.
  The worktree guard refused a `for` loop over `i18n:values` per locale and a `while read` over the brief. (cs, ru, uk,
  hu)
- The settings-window `windows` pass listed only the 61 stale keys, but nine more keys already had a `windows` branch
  carrying the macOS word (`menu.app.settings`, `fileExplorer.quickFilter.intro.openSettings`,
  `fileExplorer.functionKeyBar.hiddenToast`, `whatsNew.optOutToast`, three `onboarding.stepBeta.*`, two more); only
  `i18n-terms` surfaced two of them, and a catalog `grep` found the rest. A per-OS term ruling (`settings`' `windows`
  form) could drive a check that lists every `windows` branch still using the base form. The seeded `windows` branches
  also carried the macOS `drive` word (`disque`), flagged by the termbase only after apply. (fr)
- Copying 70 `windows` branches out of `pnpm i18n:values` with a branch swap took a one-off `gawk` extractor (my own
  values file, no catalog write), since the tool has no JSON output; a `--json` mode (key → branch → text) would have
  made it a `jq` one-liner. KDE values typed by hand needed the U+00A0 inside « » added after the mechanics check. The
  worktree guard refused a `jq … && mv` chain and an `echo >> keys.txt && jq` chain; split into single calls. (fr)
- Extending a run's batch with keys that aren't stale meant appending to my own `keys.txt` by hand; `i18n:apply` has no
  flag to add keys to the batch. (fr)
- The empty `mtp.connectedToast.body` `windows` branch needed the same `sameAsSourceJustification` in de, nl, sv, and fr
  too (see the entry above). (de, nl, sv, fr)
- Rewording one word across a branch in 66 keys (Ajustes → Configuración on `windows`) had no tool, so I wrote two
  throwaway helpers under `_ignored/i18n-work/tools/`: one dumps each key's `windows` / English `linuxKde` branch as a
  draft values file, one applies ordered find-and-replace pairs to the `windows` branch and fills `{ "keep": true }` for
  the rest. An `i18n:apply --branch windows --replace "A=>B"` (with a dry-run diff) would cover it. (es, pt)
- Android labels have no pile source: the `usb-preferences` rulings came from AOSP Settings `strings.xml`
  (`usb_preference`, `settings_label`, `usb_use_file_transfers`) fetched with `curl` from `aosp-mirror` for five
  locales. Adding AOSP Settings to `i18n:pile-fetch` would make it a lookup. (es, pt, vi, zh, zh-Hant)
- The worktree guard refused pipes and loops naming the scratchpad path (it contains `projects-git`), so the working
  files moved under `_ignored/i18n-work/`. (es)
- A branch that removed translations when it made keys per-OS left the `macos` branch to restore from git history, and
  no tool reads a key's value at a ref: a read-only script over a `git archive` of the old catalogs filled the gap, and
  retyping those values from terminal output silently dropped Czech's U+00A0 after one-letter prepositions (caught by
  `i18n-mechanics`, then a byte-compare script). An `i18n:values --ref <rev>` that prints escapes, or an
  `i18n:apply --restore-from <rev>` for the `macos` branch, would make the restore exact. (all locales, per-OS machine
  names batch)
- A two-key batch for 13 locales: `i18n:pile` and `i18n:check-locale` take one `--lang` each, so the lookups took four
  chained calls and the checks two chains of six to seven locales, and each check chain outran the Bash tool's 120 s
  foreground limit and moved to the background. A shell variable for the run directory was refused again ("names git").
  A `--lang all` on `check-locale` (reading each locale's latest run directory) and on `i18n:pile` would make each one
  call. No script written. (all locales, demo question box)
- The pile has no Disk Utility strings, so "Mac OS Extended" had no hits; it came from `plutil` + `jq` over StorageKit's
  `Localizable.loctable` on the live system. A `pnpm i18n:pile --macos-live` lookup would cover it. Also, `i18n:apply`
  takes one `--lang` and the guard refused shell loops and variables, so 13 locales meant 26 separate calls; the
  per-locale term patches were split out of one `jq` output with `awk`. (all, linksNotSupported batch)
- A three-key batch for 13 locales: `i18n:apply` still takes one `--lang`, so the guard's loop refusal meant three `&&`
  chains, and the `es-419` fork's values file sat outside any run directory again. `i18n:check-locale` with every locale
  and one `--keys-file` worked in a single call. The brief's "No concept yet" words (say, reached, left, keep) were
  plain prose verbs, so the pile wasn't mined for them. No script written. (all, cantTellIfSameFile batch)
