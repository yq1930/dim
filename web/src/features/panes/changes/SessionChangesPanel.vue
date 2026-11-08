<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue"
import { useI18n } from "vue-i18n"
import { ChevronRight, MessageSquare, Trash2 } from "lucide-vue-next"
import { useChatStore } from "../../../stores/chat"
import { useSessionsStore } from "../../../stores/sessions"
import { useUiStore, type ReviewNote } from "../../../stores/ui"
import DiffLines from "../../../components/DiffLines.vue"
import { sessionChanges, workspacePath, type FileEdit } from "./session-changes"

/**
 * The detail panel's review of what the Session's main line changed: every
 * file its `write` and `edit` calls touched, with each call's diff in order.
 * Picking a line opens a comment on it; the comments go into the main
 * composer together, as one message for the operator to send.
 */
const props = defineProps<{ sessionId: string }>()
const { t } = useI18n()
const chat = useChatStore()
const sessions = useSessionsStore()
const ui = useUiStore()

const view = computed(() => chat.views[props.sessionId])
const cwd = computed(() => sessions.byId.get(props.sessionId)?.cwd)
const files = computed(() => sessionChanges(view.value?.rows ?? [], cwd.value))
const totals = computed(() => files.value.reduce((sum, file) => ({ added: sum.added + file.added, removed: sum.removed + file.removed }), { added: 0, removed: 0 }))
/** Files the reader folded; every file starts open. */
const folded = ref<Record<string, boolean>>({})
const notes = computed(() => ui.reviewNotes[props.sessionId] ?? [])
/** The line being commented on, and the comment so far. */
const drafting = ref<{ key: string; file: string; edit: FileEdit; index: number }>()
const draftText = ref("")
const draftField = ref<HTMLTextAreaElement[]>()
watch(() => props.sessionId, () => { folded.value = {}; drafting.value = undefined })

function noteKey(edit: FileEdit, index: number): string {
  return `${edit.key}:${index}`
}

function noteAt(edit: FileEdit, index: number): ReviewNote | undefined {
  return notes.value.find((note) => note.key === noteKey(edit, index))
}

function marked(edit: FileEdit): Set<number> {
  return new Set(edit.lines.flatMap((_, index) => noteAt(edit, index) ? [index] : []))
}

function pick(path: string, edit: FileEdit, index: number): void {
  const key = noteKey(edit, index)
  drafting.value = { key, file: workspacePath(path, cwd.value), edit, index }
  draftText.value = noteAt(edit, index)?.note ?? ""
  void nextTick(() => draftField.value?.[0]?.focus())
}

function saveNote(): void {
  const draft = drafting.value
  const note = draftText.value.trim()
  if (draft === undefined) return
  if (note === "") ui.removeReviewNote(props.sessionId, draft.key)
  else {
    const line = draft.edit.lines[draft.index]!
    ui.addReviewNote(props.sessionId, { key: draft.key, file: draft.file, ...(line.line === undefined ? {} : { line: line.line }), removed: line.sign === "-", code: line.text, note })
  }
  drafting.value = undefined
}

/** One message for the composer: each comment with where it is and the line it is on. */
function reviewMessage(): string {
  const items = notes.value.map((note, index) => {
    const place = `${note.file}${note.line === undefined ? "" : `:${note.line}`}${note.removed ? t("review.removedLine") : ""}`
    const code = note.code.trim() === "" ? "" : `\n   > ${note.code.trim()}`
    return `${index + 1}. ${place} —— ${note.note}${code}`
  })
  return `${t("review.intro")}\n\n${items.join("\n")}`
}

function putIntoComposer(): void {
  if (notes.value.length === 0) return
  ui.requestComposerInsert(props.sessionId, reviewMessage())
  ui.clearReviewNotes(props.sessionId)
}
</script>

<template>
  <div class="changes-panel" role="region" :aria-label="t('sessionChanges.title')">
    <header v-if="files.length" class="changes-header">
      <span class="changes-summary">
        {{ t("sessionChanges.files", { count: files.length }, files.length) }}
        <span class="changes-stat" data-tone="success">+{{ totals.added }}</span>
        <span class="changes-stat" data-tone="danger">−{{ totals.removed }}</span>
      </span>
    </header>

    <div class="changes-body app-scroll-viewport">
      <div v-if="!view || view.status === 'loading'" class="changes-empty" role="status">{{ t("common.loading") }}</div>
      <p v-else-if="files.length === 0" class="changes-empty">{{ t("sessionChanges.empty") }}</p>
      <section v-for="file in files" :key="file.path" class="changes-file">
        <button type="button" class="changes-file-toggle" :aria-expanded="!folded[file.path]" :title="file.path" @click="folded[file.path] = !folded[file.path]">
          <ChevronRight :size="14" :class="{ 'is-expanded': !folded[file.path] }" aria-hidden="true" />
          <span class="changes-file-path">{{ workspacePath(file.path, cwd) }}</span>
          <span class="changes-stat" data-tone="success">+{{ file.added }}</span>
          <span v-if="file.removed" class="changes-stat" data-tone="danger">−{{ file.removed }}</span>
        </button>
        <div v-if="!folded[file.path]" class="changes-file-body">
          <template v-for="(edit, index) in file.edits" :key="edit.key">
            <hr v-if="index > 0" class="changes-separator">
            <DiffLines :lines="edit.lines" numbered pickable :marked="marked(edit)" @pick="pick(file.path, edit, $event)">
              <template #after="{ index: line }">
                <form v-if="drafting?.key === noteKey(edit, line)" class="review-draft" @submit.prevent="saveNote">
                  <textarea ref="draftField" v-model="draftText" class="ui-field" rows="2" :placeholder="t('review.placeholder')" :aria-label="t('review.placeholder')" @keydown.enter.exact.prevent="saveNote" @keydown.esc.stop.prevent="drafting = undefined" />
                  <div class="review-draft-actions">
                    <button type="button" class="ui-button" data-variant="ghost" data-size="sm" @click="drafting = undefined">{{ t("common.cancel") }}</button>
                    <button type="submit" class="ui-button" data-variant="secondary" data-size="sm">{{ t("review.save") }}</button>
                  </div>
                </form>
                <p v-else-if="noteAt(edit, line)" class="review-note">
                  <MessageSquare :size="12" aria-hidden="true" />
                  <span>{{ noteAt(edit, line)!.note }}</span>
                  <button type="button" class="ui-icon-button" data-variant="ghost" data-size="xs" :aria-label="t('review.remove')" v-tooltip="t('review.remove')" @click="ui.removeReviewNote(sessionId, noteKey(edit, line))"><Trash2 aria-hidden="true" /></button>
                </p>
              </template>
            </DiffLines>
            <p v-if="edit.hidden" class="changes-hidden">{{ t("sessionChanges.hiddenLines", { count: edit.hidden }, edit.hidden) }}</p>
          </template>
        </div>
      </section>
    </div>
    <footer v-if="notes.length" class="review-bar">
      <span>{{ t("review.count", { count: notes.length }, notes.length) }}</span>
      <button type="button" class="ui-button" data-variant="ghost" data-size="sm" @click="ui.clearReviewNotes(sessionId)">{{ t("review.clear") }}</button>
      <button type="button" class="ui-button" data-variant="primary" data-size="sm" @click="putIntoComposer">{{ t("review.insert") }}</button>
    </footer>
  </div>
</template>

<style scoped>
.changes-panel { display: flex; min-width: 0; min-height: 0; flex-direction: column; background: var(--background); }
.changes-header { display: flex; min-width: 0; height: 36px; flex: none; align-items: center; padding: 0 var(--space-3); border-bottom: 1px solid var(--border); font-size: var(--text-sm); }
.changes-summary { display: flex; min-width: 0; align-items: center; gap: var(--space-1); overflow: hidden; color: var(--muted-foreground); white-space: nowrap; }
.changes-body { display: flex; min-height: 0; flex: 1; flex-direction: column; gap: var(--space-2); padding: var(--space-3); overflow-y: auto; }
.changes-empty { margin: auto; color: var(--muted-foreground); font-size: var(--text-sm); }
.changes-file { flex: none; overflow: hidden; border: 1px solid var(--border); border-radius: var(--radius-lg); background: var(--card); }
.changes-file-toggle { display: flex; width: 100%; min-width: 0; height: var(--control-md); align-items: center; gap: var(--space-2); padding: 0 var(--space-3) 0 var(--space-2); border: 0; background: var(--muted); color: var(--foreground); font: inherit; font-size: var(--text-sm); text-align: left; cursor: pointer; }
.changes-file-toggle > svg { flex: none; color: var(--muted-foreground); transition: transform var(--duration-press) var(--ease-out); }
.changes-file-toggle > svg.is-expanded { transform: rotate(90deg); }
.changes-file-path { min-width: 0; flex: 1; overflow: hidden; font-family: var(--font-mono); font-size: var(--text-xs); text-overflow: ellipsis; white-space: nowrap; }
.changes-stat { flex: none; font-family: var(--font-mono); font-size: var(--text-xs); font-variant-numeric: tabular-nums; }
.changes-stat[data-tone="success"] { color: var(--success); }
.changes-stat[data-tone="danger"] { color: var(--danger); }
.changes-file-body { border-top: 1px solid var(--border); overflow-x: auto; }
.changes-separator { margin: 0; border: 0; border-top: 1px dashed var(--border); }
.changes-hidden { margin: 0; padding: 0 var(--space-3) var(--space-2); color: var(--muted-foreground); font-size: var(--text-xs); }
.review-draft { display: grid; gap: var(--space-2); margin: var(--space-1) var(--space-3) var(--space-2); font-family: var(--font-sans); }
.review-draft > .ui-field { width: 100%; resize: vertical; }
.review-draft-actions { display: flex; justify-content: flex-end; gap: var(--space-1); }
.review-note { display: flex; align-items: flex-start; gap: var(--space-2); margin: var(--space-1) var(--space-3) var(--space-2); padding: var(--space-1) var(--space-2); border-radius: var(--control-sm-radius); background: color-mix(in oklab, var(--warning) 12%, transparent); font: var(--text-sm)/var(--leading-relaxed) var(--font-sans); white-space: pre-wrap; }
.review-note > svg { flex: none; margin-top: 4px; color: var(--warning); }
.review-note > span { min-width: 0; flex: 1; overflow-wrap: anywhere; }
.review-bar { display: flex; flex: none; align-items: center; gap: var(--space-2); padding: var(--space-2) var(--space-3); border-top: 1px solid var(--border); font-size: var(--text-sm); }
.review-bar > span { margin-right: auto; color: var(--muted-foreground); }
</style>
