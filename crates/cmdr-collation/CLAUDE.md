# cmdr-collation

How two filenames rank: `NameCollator`, one ICU4X collator per locale, and `NameKey`, the prebuilt key a bulk sort
compares. ICU only, no `tauri`, no state. `cmdr-listing` re-exports `NameCollator`, so the panes, search results, and
the write engine's tree order (which names it from here, handed the panes' instance) all sort one way.

## Module map

- **lib.rs**: the whole crate. `NameCollator` (`new`, `compare`, `key`), `NameKey`, and the unit tests that pin `key`
  and `compare` to one order.

## Must-knows

- **Names collate, ❌ never by code point.** Both readings end with a raw-bytes tiebreak, so two distinct names (the NFC
  and NFD spellings of one) never tie.
- ❌ **Never persist a `NameKey`**: its bytes are ICU4X's internal encoding, which a library or CLDR update may re-tune.
- **`key` and `compare` must agree.** A change to either keeps the `a_key_ranks_names_the_way_compare_does` test green.
- **No cache here.** `NameCollator::new` loads ICU data; a caller that sorts often keeps one per locale
  (`cmdr-listing`'s `Inner::collator`).
- **A broken locale tag falls back to root**, never an error: a bad setting still sorts.
- The public surface is capped by `index-crate-isolation`; an addition is argued in `DETAILS.md`.

Why a collator, the options chosen, the `unstable` ICU feature, and the capped surface: `DETAILS.md`.
