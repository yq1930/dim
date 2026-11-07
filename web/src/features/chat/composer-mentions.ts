import { computed, nextTick, onBeforeUnmount, ref, watch, type ComputedRef, type Ref } from "vue"
import { mentionTrigger, type MentionTrigger } from "../../mentions"
import type { FileCandidate } from "../../rpc/protocol"
import type { ComposerDraft } from "./composer-drafts"

/** Typing pauses this long before the workspace is searched. */
const SEARCH_DELAY_MS = 80

/**
 * The `@` file picker: the mention the caret is in (read from the field, the
 * source of truth for the caret), the workspace files that match it, searched
 * once typing pauses with a late answer to an earlier query dropped, and the
 * chosen file inserted as `@path`, as the TUI picker does. `search` is absent
 * where the Session serves no file search.
 */
export function useMentionPicker(options: {
  field: Ref<HTMLTextAreaElement | undefined>
  draft: ComputedRef<ComposerDraft>
  focused: Ref<boolean>
  search: () => ((query: string) => Promise<FileCandidate[]>) | undefined
  resize: () => void
}) {
  const { field, draft, focused, search, resize } = options
  const mention = ref<MentionTrigger>()
  const dismissedKey = ref<string>()
  const candidates = ref<FileCandidate[]>([])
  const active = ref(0)
  let searchSeq = 0
  let searchTimer: ReturnType<typeof setTimeout> | undefined
  const key = computed(() => mention.value === undefined ? undefined : `${mention.value.start}:${mention.value.query}`)
  const open = computed(() => focused.value && search() !== undefined && mention.value !== undefined && key.value !== dismissedKey.value)
  const shown = computed(() => open.value && candidates.value.length > 0)

  function update(): void {
    const element = field.value
    mention.value = element === undefined || search() === undefined ? undefined : mentionTrigger(element.value, element.selectionStart)
  }

  watch(() => mention.value?.query, (query) => {
    const seq = ++searchSeq
    clearTimeout(searchTimer)
    active.value = 0
    const find = search()
    if (query === undefined || find === undefined) {
      candidates.value = []
      return
    }
    const target = draft.value
    searchTimer = setTimeout(() => {
      find(query).then((found) => {
        if (seq === searchSeq) candidates.value = found
      }, (cause: unknown) => {
        if (seq !== searchSeq) return
        candidates.value = []
        target.error = cause instanceof Error ? cause.message : String(cause)
      })
    }, SEARCH_DELAY_MS)
  })

  /** Inserts the chosen file as `@path` (a space after it) or steps into a directory as `@dir/`. */
  function accept(candidate: FileCandidate): void {
    const element = field.value
    const trigger = mention.value
    if (element === undefined || trigger === undefined) return
    const target = draft.value
    const cursor = element.selectionStart
    const label = `@${candidate.relativePath}${candidate.dir ? "/" : ""}`
    const separator = candidate.dir ? "" : " "
    target.text = `${target.text.slice(0, trigger.start)}${label}${separator}${target.text.slice(cursor)}`
    if (!candidate.dir) target.mentions = [...target.mentions, { label: `@${candidate.relativePath}`, path: candidate.absolutePath }]
    const caret = trigger.start + label.length + separator.length
    void nextTick(() => {
      element.setSelectionRange(caret, caret)
      resize()
      update()
    })
  }

  /** Inserts a file picked in the project-file dialog at the caret, as the picker would. */
  function insert(candidate: FileCandidate): void {
    const element = field.value
    if (!element) return
    const target = draft.value
    const start = element.selectionStart
    const end = element.selectionEnd
    const label = `@${candidate.relativePath}`
    const prefix = start > 0 && !/\s/.test(target.text[start - 1]!) ? " " : ""
    target.text = `${target.text.slice(0, start)}${prefix}${label} ${target.text.slice(end)}`
    target.mentions = [...target.mentions, { label, path: candidate.absolutePath }]
    const caret = start + prefix.length + label.length + 1
    void nextTick(() => { element.focus(); element.setSelectionRange(caret, caret); resize() })
  }

  onBeforeUnmount(() => { ++searchSeq; clearTimeout(searchTimer) })

  return {
    candidates, active, open, shown, update, accept, insert,
    move: (step: 1 | -1) => { active.value = (active.value + step + candidates.value.length) % candidates.value.length },
    /** Escape closes the picker for this mention until the caret moves to another. */
    dismiss: () => { dismissedKey.value = key.value },
    reset: () => { mention.value = undefined; candidates.value = [] },
  }
}
