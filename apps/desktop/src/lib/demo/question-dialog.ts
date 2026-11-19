/**
 * The dialog plugin's question (`plugin:dialog|message`, under `ask`, `confirm`, and `message`),
 * asked in the app's own modal (`QuestionDialog.svelte`) over the frame that asked, as the
 * desktop's native alert sits over its window. Every button the plugin's set names is there, so a
 * Yes/No/Cancel question answers all three, and nothing blocks the page the way the browser's
 * `confirm` would. A webview answers it itself (`own-window.ts`). See `DETAILS.md` § Dialogs.
 */
import { mount, unmount } from 'svelte'
import { tString } from '#lib/intl/messages.svelte.js'
import QuestionDialog from './QuestionDialog.svelte'

/** The plugin's button set, as `@tauri-apps/plugin-dialog`'s `buttonsToRust` sends it. */
type Buttons =
  | 'Ok'
  | 'OkCancel'
  | 'YesNo'
  | 'YesNoCancel'
  | { OkCustom: string }
  | { OkCancelCustom: [string, string] }
  | { YesNoCancelCustom: [string, string, string] }

/** One button: the word it shows, and the name the plugin answers when it's clicked. */
export interface QuestionButton {
  label: string
  answer: string
  /** The confirming button: Return presses it. */
  isDefault: boolean
}

/** A question as the dialog shows it. */
export interface Question {
  title: string
  message: string
  /** Left to right, the confirming one last, as the app's own dialogs order them. */
  buttons: QuestionButton[]
  /** What Escape (or a click outside) answers: the cancelling button's name, as the native alert does. */
  escapeAnswer: string
}

/** The words of the plugin's own button names; a custom set's names are its words. */
function standardLabel(answer: 'Ok' | 'Cancel' | 'Yes' | 'No'): string {
  switch (answer) {
    case 'Ok':
      return tString('ui.alertDialog.defaultButton')
    case 'Cancel':
      return tString('ui.confirmDialog.cancel')
    case 'Yes':
      return tString('ui.demoQuestion.yes')
    case 'No':
      return tString('ui.demoQuestion.no')
  }
}

/** A set's answers, the confirming one first, each with its word. */
function answersOf(buttons: Buttons | undefined): [string, string][] {
  const standard = (...names: ('Ok' | 'Cancel' | 'Yes' | 'No')[]): [string, string][] =>
    names.map((name) => [name, standardLabel(name)])
  const custom = (...words: string[]): [string, string][] => words.map((word) => [word, word])
  if (buttons === undefined || buttons === 'Ok') return standard('Ok')
  if (buttons === 'OkCancel') return standard('Ok', 'Cancel')
  if (buttons === 'YesNo') return standard('Yes', 'No')
  if (buttons === 'YesNoCancel') return standard('Yes', 'No', 'Cancel')
  if ('OkCustom' in buttons) return custom(buttons.OkCustom)
  if ('OkCancelCustom' in buttons) return custom(...buttons.OkCancelCustom)
  return custom(...buttons.YesNoCancelCustom)
}

/** The question `payload` asks, as the dialog shows it. `fallbackTitle` titles one that names none. */
export function questionOf(payload: Record<string, unknown>, fallbackTitle: string): Question {
  const answers = answersOf(payload.buttons as Buttons | undefined)
  const [confirming] = answers
  const title = typeof payload.title === 'string' && payload.title !== '' ? payload.title : fallbackTitle
  return {
    title,
    message: typeof payload.message === 'string' ? payload.message : '',
    // The confirming answer last; the rest keep their order before it (Cancel, No, then Yes).
    buttons: [...answers.slice(1).reverse(), confirming].map(([answer, label]) => ({
      label,
      answer,
      isDefault: answer === confirming[0],
    })),
    escapeAnswer: (answers.at(-1) ?? confirming)[0],
  }
}

/** `plugin:dialog|message`: asks in this frame, and answers the clicked button's name, as the plugin does. */
export function askQuestion(payload: Record<string, unknown>): Promise<string> {
  const question = questionOf(payload, document.title || 'Cmdr')
  return new Promise((resolve) => {
    const host = document.createElement('div')
    host.style.display = 'contents'
    document.body.append(host)
    let answered = false
    const component = mount(QuestionDialog, {
      target: host,
      props: {
        question,
        onAnswer: (answer: string) => {
          if (answered) return
          answered = true
          void unmount(component).then(() => {
            host.remove()
          })
          resolve(answer)
        },
      },
    })
  })
}
