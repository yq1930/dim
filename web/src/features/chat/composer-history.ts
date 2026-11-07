import { nextTick, ref, type ComputedRef, type Ref } from "vue"
import { unlinkMentions } from "../../mentions"
import type { ComposerDraft } from "./composer-drafts"

/**
 * ↑/↓ through the line's earlier messages, oldest first in `history`, as sent:
 * ↑ on the first line of an empty draft, or of one still holding a recalled
 * message, steps back; ↓ on its last line steps forward and past the newest back
 * to an empty draft. A recalled message's file links become `@path` mentions
 * again. A draft the operator wrote is never replaced.
 */
export function useHistoryRecall(options: {
  field: Ref<HTMLTextAreaElement | undefined>
  draft: ComputedRef<ComposerDraft>
  history: () => readonly string[]
  resize: () => void
}) {
  const { field, draft, history, resize } = options
  /** The `history` entry the draft holds while ↑/↓ walk it. */
  const recalled = ref<number>()

  /** Takes the key press when it moved through `history`. */
  function recall(step: -1 | 1): boolean {
    const element = field.value
    const messages = history()
    if (element === undefined || messages.length === 0) return false
    const target = draft.value
    const shown = recalled.value === undefined ? undefined : messages[recalled.value]
    const browsing = shown !== undefined && target.text === unlinkMentions(shown).text
    if (!browsing && target.text !== "") return false
    if (step < 0 && target.text.slice(0, element.selectionStart).includes("\n")) return false
    if (step > 0 && (!browsing || target.text.slice(element.selectionEnd).includes("\n"))) return false
    const next = (browsing ? recalled.value! : messages.length) + step
    if (next < 0) return true
    if (next >= messages.length) {
      recalled.value = undefined
      target.text = ""
      target.mentions = []
    } else {
      const { text, mentions } = unlinkMentions(messages[next]!)
      recalled.value = next
      target.text = text
      target.mentions = mentions
    }
    void nextTick(() => {
      const end = target.text.length
      element.setSelectionRange(end, end)
      resize()
    })
    return true
  }

  return { recall, forget: () => { recalled.value = undefined } }
}
