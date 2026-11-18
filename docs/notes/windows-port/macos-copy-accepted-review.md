# Accepted macOS copy changes: the review list

Every macOS text that moved on purpose while `macos-copy-unchanged.test.ts` froze the macOS copy (Windows port task 28,
`docs/specs/windows-port-plan.md`). **Approved by David on 2026-11-18**, all 18 entries; the guard, its baseline, and
its accepted-changes store were then deleted, so this note is the record of what moved. It was generated on 2026-11-18
from those deleted files (`apps/desktop/src/lib/intl/__fixtures__/macos-copy-accepted/*.json` for the new text and
reason, `__fixtures__/macos-copy-baseline.json` for the old text a Mac reader saw); git holds them.

- **Translation changes**: 14 entries in 10 catalogs. Eight are one English rewording carried into each language
  (`shortcuts.conflict.systemShortcut`, a pronoun aimed at `{combo}`), three are the Spanish and Vietnamese follow-ups
  of two other English rewordings, and three are plain slips (a Hungarian article in two keys, a Ukrainian missing
  space).
- **English changes**: four entries in `ACCEPTED_ENGLISH_CHANGES` (in the deleted
  `apps/desktop/scripts/i18n-macos-copy-baseline-lib.ts`). The first two are David's (`copy-decisions.md`), the other
  two translator-reported fixes (`docs/i18n/source-queue.md`).
- Out of scope here: the 16 `RETIRED_KEYS` (keys David deleted on purpose, each with its reason, in that same deleted
  file).

## Translation changes, per catalog

### `de`

- `shortcuts.conflict.systemShortcut`
  - Old: {combo} wird meist von macOS belegt ({label}) und erreicht Cmdr daher womöglich nie. Du kannst sie in
    Systemeinstellungen > Tastatur freigeben.
  - New: {combo} wird meist von macOS belegt ({label}) und erreicht Cmdr daher womöglich nie. Du kannst den Kurzbefehl
    in Systemeinstellungen > Tastatur freigeben.
  - Reason: „Du kannst sie … freigeben“ pointed the pronoun `sie` at `{combo}`, whose gender is unknown; it now names
    the noun, as the reworded English asks.

### `es`

- `errors.write.trashRefused.message.ownedByAnotherUser`
  - Old: Otra cuenta de este Mac posee {count} de los elementos que has elegido, y en su carpeta solo el propietario de
    un elemento puede moverlo.
  - New: Otra cuenta de este Mac posee {count} de los elementos que has elegido, y en la carpeta que los contiene solo
    el propietario de un elemento puede moverlo.
  - Reason: “en su carpeta” read as the other account’s folder; it now says the folder holding the items, as the
    reworded English does.
- `shortcuts.conflict.systemShortcut`
  - Old: macOS suele usar {combo} ({label}), así que puede que nunca llegue a Cmdr. Puedes liberarla en Ajustes del
    Sistema > Teclado.
  - New: macOS suele usar {combo} ({label}), así que puede que el atajo nunca llegue a Cmdr. Puedes liberarlo en Ajustes
    del Sistema > Teclado.
  - Reason: “Puedes liberarla” pointed a feminine pronoun at `{combo}`, whose gender is unknown; it now names “el
    atajo”, as the reworded English asks.

### `es-419`

- `errors.write.trashRefused.message.ownedByAnotherUser`
  - Old: Otra cuenta de este Mac posee {count} de los elementos que elegiste, y en su carpeta solo el propietario de un
    elemento puede moverlo.
  - New: Otra cuenta de este Mac posee {count} de los elementos que elegiste, y en la carpeta que los contiene solo el
    propietario de un elemento puede moverlo.
  - Reason: “en su carpeta” read as the other account’s folder; it now says the folder holding the items, as the
    reworded English does.
- `shortcuts.conflict.systemShortcut`
  - Old: macOS suele usar {combo} ({label}), así que puede que nunca llegue a Cmdr. Puedes liberarla en Configuración
    del Sistema > Teclado.
  - New: macOS suele usar {combo} ({label}), así que puede que el atajo nunca llegue a Cmdr. Puedes liberarlo en
    Configuración del Sistema > Teclado.
  - Reason: “Puedes liberarla” pointed a feminine pronoun at `{combo}`, whose gender is unknown; it now names “el
    atajo”, as the reworded English asks.

### `fr`

- `shortcuts.conflict.systemShortcut`
  - Old: {combo} est généralement pris par macOS ({label}), donc il pourrait ne jamais atteindre Cmdr. Vous pouvez le
    libérer dans Réglages Système > Clavier.
  - New: {combo} est généralement pris par macOS ({label}), donc ce raccourci pourrait ne jamais atteindre Cmdr. Vous
    pouvez le libérer dans Réglages Système > Clavier.
  - Reason: « donc il pourrait » pointed the pronoun `il` at `{combo}`, whose gender is unknown; it now names the noun
    (« ce raccourci »), as the reworded English asks and the Linux and menu-bar siblings already do.

### `hu`

- `fileOperations.sourcesGone.message` (only the `move` arm changed; the other four arms are identical)
  - Old (move arm): A áthelyezésre váró {totalText} elemből {goneText} … máshová került vagy törlődött, amíg a művelet a
    sorára várt.
  - New (move arm): Az áthelyezésre váró {totalText} elemből {goneText} … máshová került vagy törlődött, amíg a művelet
    a sorára várt.
  - Reason: The move arm said „A áthelyezésre”: the article before a vowel is „az”, a plain grammar slip.
- `fileOperations.sourcesGone.nothingLeftToast` (only the `move` arm changed)
  - Old (move arm): Nincs mit áthelyezni: {total, plural, one {a áthelyezésre váró elem} other {a áthelyezésre váró
    összes elem ({totalText})}} máshová került vagy törlődött, amíg a művelet a sorára várt.
  - New (move arm): Nincs mit áthelyezni: {total, plural, one {az áthelyezésre váró elem} other {az áthelyezésre váró
    összes elem ({totalText})}} máshová került vagy törlődött, amíg a művelet a sorára várt.
  - Reason: The move arm said „a áthelyezésre” twice: the article before a vowel is „az”, a plain grammar slip.

### `nl`

- `shortcuts.conflict.systemShortcut`
  - Old: {combo} wordt meestal door macOS gebruikt ({label}), dus bereikt het Cmdr mogelijk nooit. Je kunt het vrijmaken
    in Systeeminstellingen > Toetsenbord.
  - New: {combo} wordt meestal door macOS gebruikt ({label}), dus bereikt de sneltoets Cmdr mogelijk nooit. Je kunt hem
    vrijmaken in Systeeminstellingen > Toetsenbord.
  - Reason: “dus bereikt het Cmdr” and “Je kunt het vrijmaken” pointed the pronoun `het` at `{combo}`; it now names “de
    sneltoets”, as the reworded English asks.

### `pt`

- `shortcuts.conflict.systemShortcut`
  - Old: {combo} costuma ser usado pelo macOS ({label}), então pode nunca chegar ao Cmdr. Você pode liberá-lo em Ajustes
    do Sistema > Teclado.
  - New: O macOS costuma usar {combo} ({label}), então o atalho pode nunca chegar ao Cmdr. Você pode liberar o atalho em
    Ajustes do Sistema > Teclado.
  - Reason: “costuma ser usado” and “liberá-lo” agreed in the masculine with `{combo}`, whose gender is unknown; it now
    names “o atalho”, as the reworded English asks.

### `sv`

- `shortcuts.conflict.systemShortcut`
  - Old: {combo} brukar vara upptagen av macOS ({label}), så den når kanske aldrig fram till Cmdr. Du kan frigöra den i
    Systeminställningar > Tangentbord.
  - New: {combo} brukar vara upptagen av macOS ({label}), så genvägen når kanske aldrig fram till Cmdr. Du kan frigöra
    den i Systeminställningar > Tangentbord.
  - Reason: “så den når” pointed a gendered pronoun at `{combo}`; it now names “genvägen”, as the reworded English asks.

### `uk`

- `onboarding.stepBeta.signup.unreachable`
  - Old: Зараз не вдалося звʼязатися із сервером підписки, тож вас ще немає в списку розсилки.Вашу адресу збережено на
    цьому Mac: натисніть «Зберегти» ще раз за хвилину або будь-коли в Параметрах › Оновлення та приватність.
  - New: Зараз не вдалося звʼязатися із сервером підписки, тож вас ще немає в списку розсилки. Вашу адресу збережено на
    цьому Mac: натисніть «Зберегти» ще раз за хвилину або будь-коли в Параметрах › Оновлення та приватність.
  - Reason: The space after the full stop was missing («розсилки.Вашу»).

### `vi`

- `ai.secretError.genericBody`
  - Old: Kiểm tra xem kho khóa hệ thống của bạn có sẵn sàng không, rồi thử lại. Nếu vẫn tiếp diễn, hãy khởi động lại ứng
    dụng.
  - New: Kiểm tra xem chuỗi khóa của bạn có sẵn sàng không, rồi thử lại. Nếu vẫn tiếp diễn, hãy khởi động lại ứng dụng.
  - Reason: The macOS branch carried the old generic text (kho khóa hệ thống, the Linux keyring word); its English now
    names the Keychain, which vi macOS calls chuỗi khóa.
- `shortcuts.conflict.systemShortcut`
  - Old: {combo} thường được macOS dùng ({label}), nên nó có thể không bao giờ đến được Cmdr. Bạn có thể giải phóng nó
    trong Cài đặt hệ thống > Bàn phím.
  - New: {combo} thường được macOS dùng ({label}), nên phím tắt này có thể không bao giờ đến được Cmdr. Bạn có thể giải
    phóng phím tắt này trong Cài đặt hệ thống > Bàn phím.
  - Reason: “nên nó” could read as macOS (the nearer subject) rather than `{combo}`; it now names “phím tắt này”, as the
    reworded English asks.

## English changes (`ACCEPTED_ENGLISH_CHANGES`)

- `ai.secretError.genericBody` (David's)
  - Old: Check that your system keyring is available, then try again. If it persists, restart the app.
  - New: Check that your Keychain is available, then try again. If it persists, restart the app.
- `goToPath.dialog.inputPlaceholder` (David's)
  - Old: Type or paste a path, e.g. ~/Documents
  - New: Type or paste a path, for example ~/Documents
- `errors.write.trashRefused.message.ownedByAnotherUser` (translator-reported)
  - Old: Another account on this Mac owns {count} of the items you picked, and their folder lets only an item’s owner
    move it.
  - New: Another account on this Mac owns {count} of the items you picked, and the folder holding them lets only an
    item’s owner move it.
- `shortcuts.conflict.systemShortcut` (translator-reported)
  - Old: {combo} is usually taken by macOS ({label}), so it may never reach Cmdr. You can free it up in System
    Settings > Keyboard.
  - New: {combo} is usually taken by macOS ({label}), so the shortcut may never reach Cmdr. You can free it up in System
    Settings > Keyboard.

## Followed after the guard went (2026-11-18)

- **`ai.secretError.genericBody`**: the English macOS text names the Keychain, and 12 catalogs read the old "system
  keyring" wording on macOS (`cs`, `de`, `es`, `fr`, `hu`, `nl`, `pt`, `ru`, `sv`, `uk`, `zh`, `zh-Hant`; `es-419`
  inherits `es`). Each now uses its `terms.json` `keychain` ruling, as a plain translation edit.
- **`main.quit.countdown`** in `es` (and `es-419`, which inherits it): the block has no `macos` branch, so macOS and
  Linux both read `other`, and the Spanish `other` said "al reiniciar el Mac", wrong on Linux. It now names no OS, like
  the English `other` ("a restart or logout").
