/**
 * How this platform's own apps behave (dialog button order, the keys that commit and cancel a
 * dialog, access keys, the menu model, who draws the context menus, the screen readers to expect, and on Linux the desktop), as
 * the backend says, read synchronously. A UI primitive asks a convention, ❌ never "is this
 * Windows", so Linux follows its desktop with no extra branch. Mechanics: `DETAILS.md` § Conventions.
 */
import type {
  AccessKeys,
  CancelKeys,
  ContextMenus,
  DialogButtonOrder,
  EnterActivates,
  HiddenMenuBar,
  LinuxDesktop,
  MenuModel,
  PlatformConventions,
  ScreenReader,
} from '#lib/ipc/bindings.js'
import conventionsOn from './conventions-on.json'

// Each convention's own type (`DialogButtonOrder`, …) stays in `#lib/ipc/bindings.js` for whoever names one.
export type { PlatformConventions }

/** A desktop with its own row of conventions: each OS, and each Linux desktop. */
export type ConventionsDesktop = 'macos' | 'windows' | 'gnome' | 'kde' | 'otherLinux'

/**
 * Every desktop's conventions, the backend's `conventions_on` table as JSON. A Rust test pins this
 * file to that table, so it can't drift; the browser demo and the test helpers read it.
 */
export const CONVENTIONS_ON = conventionsOn as Readonly<Record<ConventionsDesktop, Readonly<PlatformConventions>>>

/** The global the backend's init script defines beside the capabilities. Tests set it via `withConventions()`. */
export const CONVENTIONS_GLOBAL = '__CMDR_CONVENTIONS__'

/**
 * What a context without a backend runs as: the fallback platform's (`'linux'`) desktop-neutral
 * row, so the two fallbacks agree.
 */
export const FALLBACK_CONVENTIONS: Readonly<PlatformConventions> = Object.freeze({ ...CONVENTIONS_ON.otherLinux })

/** Lists every member of a string union, and fails to compile when one is missing. */
function every<T extends string>() {
  return <const L extends readonly T[]>(...values: L & ([T] extends [L[number]] ? unknown : never)): readonly T[] =>
    values
}

const DIALOG_BUTTON_ORDERS = every<DialogButtonOrder>()('cancelFirst', 'affirmativeFirst')
const ENTER_ACTIVATES = every<EnterActivates>()('defaultButton', 'focusedButton')
const CANCEL_KEYS = every<CancelKeys>()('escape', 'escapeOrCommandPeriod')
const ACCESS_KEYS = every<AccessKeys>()('none', 'underlinesAfterKeyboardUse', 'underlinesWhileAltHeld')
const MENU_MODELS = every<MenuModel>()('systemMenuBar', 'windowMenuBar', 'primaryMenu')
const HIDDEN_MENU_BARS = every<HiddenMenuBar>()('none', 'showsOnAltOrF10', 'f10OpensMainMenu')
const CONTEXT_MENUS = every<ContextMenus>()('native', 'inPage')
const SCREEN_READERS = every<ScreenReader>()('voiceOver', 'jaws', 'nvda', 'narrator', 'orca')
const LINUX_DESKTOPS = every<LinuxDesktop>()('gnome', 'kde', 'other')

function oneOf<T extends string>(allowed: readonly T[], value: unknown, fallback: T): T {
  return allowed.includes(value as T) ? (value as T) : fallback
}

/**
 * How this platform's apps behave, as the backend says. Each field reads the injected value where
 * it names a known member, else the fallback's, so a missing global or a junk field can't hand a
 * primitive a value it has no branch for.
 */
export function platformConventions(): PlatformConventions {
  const injected = (globalThis as Record<string, unknown>)[CONVENTIONS_GLOBAL]
  const answered = (typeof injected === 'object' && injected !== null ? injected : {}) as Partial<
    Record<keyof PlatformConventions, unknown>
  >
  const fallback = FALLBACK_CONVENTIONS
  const readers: unknown = answered.screenReaders
  return {
    dialogButtonOrder: oneOf(DIALOG_BUTTON_ORDERS, answered.dialogButtonOrder, fallback.dialogButtonOrder),
    enterActivates: oneOf(ENTER_ACTIVATES, answered.enterActivates, fallback.enterActivates),
    cancelKeys: oneOf(CANCEL_KEYS, answered.cancelKeys, fallback.cancelKeys),
    accessKeys: oneOf(ACCESS_KEYS, answered.accessKeys, fallback.accessKeys),
    menuModel: oneOf(MENU_MODELS, answered.menuModel, fallback.menuModel),
    hiddenMenuBar: oneOf(HIDDEN_MENU_BARS, answered.hiddenMenuBar, fallback.hiddenMenuBar),
    contextMenus: oneOf(CONTEXT_MENUS, answered.contextMenus, fallback.contextMenus),
    screenReaders:
      Array.isArray(readers) &&
      readers.length > 0 &&
      (readers as unknown[]).every((reader) => SCREEN_READERS.includes(reader as ScreenReader))
        ? [...(readers as ScreenReader[])]
        : [...fallback.screenReaders],
    linuxDesktop: validLinuxDesktop(answered.linuxDesktop),
  }
}

function validLinuxDesktop(value: unknown): LinuxDesktop | null {
  return value === null || LINUX_DESKTOPS.includes(value as LinuxDesktop)
    ? (value as LinuxDesktop | null)
    : FALLBACK_CONVENTIONS.linuxDesktop
}

/**
 * The Linux desktop alone (`convention('linuxDesktop')`), without building the whole set: the
 * message runtime asks it on every `t()` on Linux, to pick a platform block's desktop branch.
 */
export function linuxDesktop(): LinuxDesktop | null {
  const injected = (globalThis as Record<string, unknown>)[CONVENTIONS_GLOBAL]
  if (typeof injected !== 'object' || injected === null) return FALLBACK_CONVENTIONS.linuxDesktop
  return validLinuxDesktop((injected as Partial<Record<keyof PlatformConventions, unknown>>).linuxDesktop)
}

/** One convention of this platform, for example `convention('dialogButtonOrder')`. */
export function convention<K extends keyof PlatformConventions>(key: K): PlatformConventions[K] {
  return platformConventions()[key]
}

/**
 * Whether the page draws the app's menu bar: everywhere but macOS, whose bar is the system's
 * (D5, task 63 of `docs/specs/windows-port-plan.md`): as a bar, or in a main-menu button where the
 * desktop or the "Show menu bar" setting hides it (`#lib/ui/command-menu/menu-layout.ts`).
 */
export function pageDrawsMenuBar(): boolean {
  return convention('menuModel') !== 'systemMenuBar'
}
