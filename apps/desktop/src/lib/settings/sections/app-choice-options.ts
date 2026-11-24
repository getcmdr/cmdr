/**
 * What every "pick an app" row shares, whatever the app is for: the row shape and
 * the "Choose an app…" sentinel. `AppChoiceSelect.svelte` renders the rows; each
 * row's own `*-options.ts` builds them from its backend answer.
 */

import type { AppFile } from '#lib/ipc/bindings.js'

/**
 * One dropdown row. Structurally a `SelectItem` (`#lib/ui/Select.svelte`), spelled
 * out here rather than imported: this module is plain TypeScript, and a type
 * reaching out of a `.svelte` module block doesn't resolve for the TS-aware lint
 * pass over `.ts` files.
 */
export interface AppChoiceOption {
  value: string
  label: string
  iconUrl?: string
}

/**
 * The "Choose an app…" row's value. Never stored: the row intercepts it and
 * opens the app picker instead. It's neither a bundle id, nor an absolute path,
 * nor `system`, the shapes Rust's `parse_choice` reads, so it can't be mistaken
 * for a real choice even if it somehow reached the store.
 */
export const CHOOSE_APP_VALUE = '__choose_app__'

/** Where the app picker starts and which files it lets through. */
export interface AppPickerShape {
  /** The extensions an app file carries, or `null` where none names one (no filter). */
  extensions: string[] | null
  /** The folder the picker opens in, or `undefined` to leave it to the OS. */
  defaultPath: string | undefined
}

/**
 * The picker for this platform's kind of app (`convention('appFile')`), so neither row
 * names an OS: macOS picks a `.app` bundle from `/Applications`, Windows an `.exe`
 * from wherever the dialog last was, Linux any file.
 * @param appFile - What an app is on disk here.
 */
export function appPickerShape(appFile: AppFile): AppPickerShape {
  switch (appFile) {
    case 'appBundle':
      return { extensions: ['app'], defaultPath: '/Applications' }
    case 'exe':
      return { extensions: ['exe'], defaultPath: undefined }
    case 'executable':
      return { extensions: null, defaultPath: undefined }
  }
}
