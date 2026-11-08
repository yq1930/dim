<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, useId, watch } from "vue"
import { useI18n } from "vue-i18n"
import { FileText, Folder, Search } from "lucide-vue-next"
import type { TextMatch, TextSearch } from "../rpc/files"
import type { FileCandidate } from "../rpc/protocol"

/**
 * Finds a file of the workspace by name. With `grep`, a query that starts with
 * `?` searches the files' contents instead and offers the lines it finds.
 */
const props = defineProps<{ search: (query: string) => Promise<FileCandidate[]>; grep?: (query: string) => Promise<TextSearch>; title?: string }>()
const emit = defineEmits<{ select: [file: FileCandidate]; match: [found: TextMatch] }>()
const { t } = useI18n()
const id = useId()
const dialog = ref<HTMLDialogElement>()
const field = ref<HTMLInputElement>()
const query = ref("")
const candidates = ref<FileCandidate[]>([])
const found = ref<TextSearch>()
const loading = ref(false)
const error = ref("")
let sequence = 0
let timer: ReturnType<typeof setTimeout> | undefined

/** The text to search the contents for, when the query asks for that. */
const contents = computed(() => props.grep !== undefined && query.value.startsWith("?") ? query.value.slice(1).trim() : undefined)
/** The content matches by file, in the order found, each line split around its match. */
const groups = computed(() => {
  const byFile = new Map<string, Array<TextMatch & { parts: ReturnType<typeof parts> }>>()
  for (const match of found.value?.matches ?? []) byFile.set(match.path, [...(byFile.get(match.path) ?? []), { ...match, parts: parts(match.text) }])
  return [...byFile].map(([path, matches]) => ({ path, matches }))
})

async function load(): Promise<void> {
  const request = ++sequence
  const text = contents.value
  loading.value = true
  error.value = ""
  try {
    if (text !== undefined) {
      const result = text === "" ? undefined : await props.grep!(text)
      if (request === sequence) found.value = result
    } else {
      const result = await props.search(query.value)
      if (request === sequence) candidates.value = result
    }
  } catch (cause) {
    if (request === sequence) error.value = cause instanceof Error ? cause.message : String(cause)
  } finally {
    if (request === sequence) loading.value = false
  }
}
watch(query, () => {
  ++sequence
  clearTimeout(timer)
  loading.value = true
  candidates.value = []
  found.value = undefined
  timer = setTimeout(() => void load(), contents.value === undefined ? 150 : 300)
})
async function open(): Promise<void> {
  query.value = ""
  candidates.value = []
  found.value = undefined
  dialog.value?.showModal()
  await nextTick()
  clearTimeout(timer)
  field.value?.focus()
  void load()
}
function close(): void { ++sequence; clearTimeout(timer); loading.value = false; dialog.value?.close() }
function select(candidate: FileCandidate): void {
  if (candidate.dir) { query.value = `${candidate.relativePath}/`; return }
  emit("select", candidate)
  close()
}
function choose(match: TextMatch): void {
  emit("match", match)
  close()
}

/** A matched line around its first match of the searched text, as the search's smart case finds it. */
function parts(text: string): { before: string; hit: string; after: string } {
  const wanted = contents.value ?? ""
  const caseless = wanted === wanted.toLowerCase()
  const at = caseless ? text.toLowerCase().indexOf(wanted) : text.indexOf(wanted)
  const line = text.trimEnd()
  if (at < 0 || wanted === "") return { before: line, hit: "", after: "" }
  const start = Math.max(0, at - 40)
  return { before: `${start > 0 ? "…" : ""}${line.slice(start, at).trimStart()}`, hit: line.slice(at, at + wanted.length), after: line.slice(at + wanted.length) }
}
onBeforeUnmount(close)
defineExpose({ open, close })
</script>

<template>
  <dialog ref="dialog" class="ui-dialog file-dialog" data-size="lg" :aria-labelledby="id" @cancel.prevent="close" @close="close">
    <header class="ui-dialog-header"><h2 :id="id" class="ui-dialog-title">{{ title ?? t("composer.workspaceFiles") }}</h2></header>
    <div class="file-search ui-field-wrap"><Search aria-hidden="true" /><input ref="field" v-model="query" class="ui-field" :placeholder="grep ? t('files.findPlaceholder') : t('composer.searchFiles')" :aria-label="t('composer.searchFiles')" /></div>
    <div class="file-results">
      <p v-if="loading" role="status">{{ t("common.loading") }}</p>
      <p v-else-if="error" class="ui-banner" data-tone="danger" role="alert">{{ error }}</p>
      <template v-else-if="contents !== undefined">
        <p v-if="contents === ''" role="status">{{ t("files.grepHint") }}</p>
        <p v-else-if="!groups.length" role="status">{{ t("files.noMatches") }}</p>
        <section v-for="group in groups" :key="group.path" class="match-group">
          <h3 class="match-file" :title="group.path">{{ group.path }}</h3>
          <button v-for="match in group.matches" :key="`${match.line}:${match.column}`" type="button" class="ui-menu-item match-row" @click="choose(match)">
            <span class="match-line">{{ match.line }}</span>
            <span class="match-text">{{ match.parts.before }}<mark>{{ match.parts.hit }}</mark>{{ match.parts.after }}</span>
          </button>
        </section>
        <p v-if="found?.truncated" class="match-more">{{ t("files.moreMatches") }}</p>
      </template>
      <template v-else>
        <p v-if="!candidates.length" role="status">{{ t("composer.noFiles") }}</p>
        <button v-for="candidate in candidates" :key="candidate.absolutePath" type="button" class="ui-menu-item" @click="select(candidate)"><Folder v-if="candidate.dir" aria-hidden="true" /><FileText v-else aria-hidden="true" /><span>{{ candidate.relativePath }}</span></button>
      </template>
    </div>
    <footer class="ui-dialog-footer"><button type="button" class="ui-button" data-variant="secondary" @click="close">{{ t("common.close") }}</button></footer>
  </dialog>
</template>

<style scoped>
.file-search { margin: var(--space-3) var(--space-4); }
.file-results { min-height: 120px; max-height: 50dvh; overflow: auto; padding: 0 var(--space-4); }
.file-results p { padding: var(--space-2); color: var(--muted-foreground); }
.file-results .ui-menu-item span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: var(--font-mono); }
.match-group + .match-group { margin-top: var(--space-2); }
.match-file { margin: 0; padding: var(--space-1) var(--space-2); overflow: hidden; color: var(--muted-foreground); font: var(--weight-medium) var(--text-xs)/1.5 var(--font-mono); text-overflow: ellipsis; white-space: nowrap; }
.match-row { gap: var(--space-3); }
.file-results .match-line { flex: none; min-width: 3ch; color: var(--muted-foreground); font-size: var(--text-xs); text-align: right; }
.file-results .match-text { min-width: 0; flex: 1; }
.match-text mark { border-radius: 2px; background: color-mix(in oklab, var(--warning) 35%, transparent); color: inherit; }
.file-results .match-more { font-size: var(--text-xs); }
</style>
