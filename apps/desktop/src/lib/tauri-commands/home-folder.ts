// The user's home folder, as the backend spells it. Its own module so the test setup can stand
// one in for every suite (`src/test-setup.ts`), as it did for Tauri's `homeDir`.

import { commands } from '#lib/ipc/bindings.js'

let homeFolder: Promise<string> | undefined

/**
 * The user's home folder as an app path (`C:/Users/test_user_1` on Windows), with no trailing `/`,
 * or `''` when the OS names none: what a `~` expands against. ❌ Never `@tauri-apps/api/path`'s
 * `homeDir`, whose native `C:\Users\…` is no app path on Windows (#1292). Asked once; every caller
 * shares the answer.
 */
export function getHomeFolder(): Promise<string> {
  homeFolder ??= commands.getHomeFolder().then(
    (home) => home ?? '',
    (error: unknown) => {
      // A broken bridge isn't an answer: the next caller asks again.
      homeFolder = undefined
      throw error
    },
  )
  return homeFolder
}
