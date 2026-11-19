/**
 * The demo's favorites, in memory: the volume list's favorite rows (`volumes.ts` lists them), and
 * the five commands that edit them, with the desktop store's rules
 * (`src-tauri/src/favorites/store.rs`: a re-add of the same folder moves it to the end, a letter
 * has one owner, a reorder never drops an entry). Gone on reload, like the rest of the demo.
 * Every edit tells the windows the list changed, as the desktop's `emit_volumes_changed` does.
 */
import { refuse, type AnswerTable, type CommandName, type Payload } from './answer-types.js'
import { DEMO_HOME } from './demo-tree.js'

/** One favorite row's own facts; `volumes.ts` dresses it as a `LocationInfo`. */
export interface DemoFavorite {
  id: string
  name: string
  path: string
  shortcut: string | null
}

const seeded = (): DemoFavorite[] => [
  { id: 'fav-home', name: 'you', path: DEMO_HOME, shortcut: 'h' },
  { id: 'fav-desktop', name: 'Desktop', path: `${DEMO_HOME}/Desktop`, shortcut: 'd' },
  { id: 'fav-documents', name: 'Documents', path: `${DEMO_HOME}/Documents`, shortcut: 'o' },
  { id: 'fav-downloads', name: 'Downloads', path: `${DEMO_HOME}/Downloads`, shortcut: 'w' },
]

let favorites = seeded()
let nextId = 1

/** The favorites in order. */
export function demoFavorites(): readonly DemoFavorite[] {
  return favorites
}

/** Back to the seeded four (tests). */
export function resetDemoFavorites(): void {
  favorites = seeded()
}

/** `at` is a pane's own path (the app favorites the folder a pane shows or its cursor names): absolute, no `~`. */
function add(at: string, name: string | null): void {
  const index = favorites.findIndex((favorite) => favorite.path === at)
  if (index >= 0) {
    const [existing] = favorites.splice(index, 1)
    favorites.push(name === null ? existing : { ...existing, name })
    return
  }
  const fallback = at === '/' ? '/' : at.slice(at.lastIndexOf('/') + 1)
  favorites.push({ id: `fav-added-${String(nextId++)}`, name: name ?? fallback, path: at, shortcut: null })
}

function setShortcut(id: string, letter: string | null): void {
  if (letter !== null && !/^[A-Za-z]$/.test(letter)) refuse('setFavoriteShortcut', { type: 'invalidLetter' })
  const shortcut = letter?.toUpperCase() ?? null
  const owns = (favorite: DemoFavorite) => favorite.shortcut?.toUpperCase() === shortcut
  if (!favorites.some((favorite) => favorite.id === id)) return
  favorites = favorites.map((favorite) => {
    if (favorite.id === id) return { ...favorite, shortcut }
    return shortcut !== null && owns(favorite) ? { ...favorite, shortcut: null } : favorite
  })
}

function reorder(orderedIds: string[]): void {
  const byId = new Map(favorites.map((favorite) => [favorite.id, favorite]))
  const ordered = orderedIds.flatMap((id) => byId.get(id) ?? [])
  favorites = [...new Set([...ordered, ...favorites])]
}

/** The favorites commands, each calling `changed` after its edit. */
export function favoriteAnswers(changed: () => void): AnswerTable {
  const editing =
    (edit: (payload: Payload) => void) =>
    (payload: Payload): null => {
      edit(payload)
      changed()
      return null
    }
  return {
    addFavorite: editing((p) => {
      add(String(p.path), (p.name as string | null) ?? null)
    }),
    removeFavorite: editing((p) => {
      favorites = favorites.filter((favorite) => favorite.id !== p.id)
    }),
    renameFavorite: editing((p) => {
      favorites = favorites.map((favorite) => (favorite.id === p.id ? { ...favorite, name: String(p.name) } : favorite))
    }),
    setFavoriteShortcut: editing((p) => {
      setShortcut(String(p.id), (p.shortcut as string | null) ?? null)
    }),
    reorderFavorites: editing((p) => {
      reorder(p.orderedIds as string[])
    }),
  }
}

/** The commands `favoriteAnswers` answers, for the demo's coverage. */
export const FAVORITE_COMMANDS = [
  'addFavorite',
  'removeFavorite',
  'renameFavorite',
  'setFavoriteShortcut',
  'reorderFavorites',
] as const satisfies readonly CommandName[]
