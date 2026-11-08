<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, provide, ref, shallowRef, watch } from "vue"
import { useI18n } from "vue-i18n"
import { Check, Code, Copy, ExternalLink, Eye, FolderOpen, MessageSquarePlus, Pencil, RefreshCw, WrapText, X } from "lucide-vue-next"
import { codeToTokens } from "../../../vendor/shiki-web"
import { copyText } from "../../../clipboard"
import ConfirmDialog from "../../../components/ConfirmDialog.vue"
import { canChooseWorkspace, openWorkspacePath } from "../../../desktop"
import { keyText, shortcutContext } from "../../../app/shortcuts"
import { normalize, useFilesStore, type OpenFile } from "../../../stores/files"
import { useToastStore } from "../../../stores/toast"
import { useUiStore } from "../../../stores/ui"
import ChatMarkdown from "../../../components/ChatMarkdown.vue"
import { LINK_CONTEXT } from "../../../components/link-context"
import FileEditor from "./FileEditor.vue"
import { fileLanguage } from "./file-language"
import { selectionQuote } from "./selection-quote"

/**
 * One open file of the detail panel's file view: its numbered lines, a Markdown
 * or SVG preview, or its image. An editable text file can be edited in place and
 * saved; selected lines can be added to the Session's message.
 */
const props = defineProps<{ sessionId: string; file: OpenFile }>()
const { t } = useI18n()
const files = useFilesStore()
const toast = useToastStore()
const ui = useUiStore()
const code = ref<HTMLElement>()
const scroller = ref<HTMLElement>()
const editor = ref<InstanceType<typeof FileEditor>>()
const confirm = ref<InstanceType<typeof ConfirmDialog>>()
const view = computed(() => props.file.view)
const text = computed(() => view.value?.kind === "text" ? view.value.text : undefined)
const label = computed(() => view.value?.relativePath ?? props.file.path)
const markdown = computed(() => /\.(md|markdown|mdx)$/i.test(props.file.path))
const previewable = computed(() => text.value !== undefined && (markdown.value || /\.svg$/i.test(props.file.path)))
const mode = ref<"preview" | "source">(previewable.value && props.file.target === undefined ? "preview" : "source")
const editable = computed(() => view.value?.kind === "text" && view.value.editable && files.supports(props.sessionId, "session.files.save"))
const editing = computed(() => props.file.editing === true && editable.value)
const editorText = computed(() => props.file.draft ?? normalize(text.value ?? ""))
const dirty = computed(() => props.file.draft !== undefined)
const saveKey = keyText("cmd+s", shortcutContext().mac)
const highlighted = shallowRef<string[]>()
const HIGHLIGHT_MAX_CHARS = 256 * 1024
let job = 0
let scrolled: number | undefined

provide(LINK_CONTEXT, computed(() => ({ sessionId: props.sessionId, base: props.file.path.slice(0, props.file.path.lastIndexOf("/")) || "/", filePane: true })))

const lines = computed(() => {
  const value = text.value
  if (value === undefined) return []
  const split = value.split(/\r\n|\r|\n/)
  if (split.length > 1 && split.at(-1) === "") split.pop()
  return split
})

function escapeHtml(value: string): string {
  return value.replace(/[&<>"]/g, (ch) => ch === "&" ? "&amp;" : ch === "<" ? "&lt;" : ch === ">" ? "&gt;" : "&quot;")
}

const html = computed(() => {
  const rich = highlighted.value
  return lines.value.map((line, index) => `<div class="file-line" data-n="${index + 1}"><span class="file-text">${rich?.[index] ?? escapeHtml(line)}</span></div>`).join("")
})
const svgSource = computed(() => text.value === undefined ? "" : `data:image/svg+xml;charset=utf-8,${encodeURIComponent(text.value)}`)

watch(text, async (value) => {
  const request = ++job
  highlighted.value = undefined
  const lang = fileLanguage(props.file.path)
  if (value === undefined || lang === undefined || value.length > HIGHLIGHT_MAX_CHARS) return
  try {
    const { tokens } = await codeToTokens(value, { lang, themes: { light: "vitesse-light", dark: "vitesse-dark" }, defaultColor: false })
    if (request !== job) return
    highlighted.value = tokens.map((line) => line.map((token) => {
      const style = Object.entries(token.htmlStyle ?? {}).map(([key, color]) => `${key}:${color}`).join(";")
      return `<span style="${escapeHtml(style)}">${escapeHtml(token.content)}</span>`
    }).join(""))
  } catch {
    // An unbundled grammar leaves the file plain.
  }
}, { immediate: true })

watch(() => props.file.target?.seq, (seq) => { if (seq !== undefined) mode.value = "source" })
watch(editing, (now) => {
  if (now) mode.value = "source"
  selected.value = undefined
})

watch([html, () => props.file.target?.seq, mode], async () => {
  await nextTick()
  const container = code.value
  if (container === undefined) return
  for (const row of container.querySelectorAll("[data-target]")) row.removeAttribute("data-target")
  const target = props.file.target
  if (target === undefined) return
  const rows = container.children
  for (let n = target.line; n <= (target.endLine ?? target.line) && n <= rows.length; n++) rows[n - 1]?.setAttribute("data-target", "")
  const row = rows[Math.min(target.line, rows.length) - 1]
  if (scrolled === target.seq || !(row instanceof HTMLElement) || scroller.value === undefined) return
  scrolled = target.seq
  scroller.value.scrollTop = Math.max(0, row.offsetTop - (scroller.value.clientHeight - row.offsetHeight) / 2)
}, { immediate: true, flush: "post" })

function failure(cause: unknown): string {
  return cause instanceof Error && cause.message ? cause.message : String(cause)
}

async function openExternally(reveal: boolean): Promise<void> {
  try {
    await openWorkspacePath(props.file.path, reveal)
  } catch (cause) {
    toast.push({ tone: "danger", title: t("chat.openFailed"), description: failure(cause) })
  }
}

async function copyPath(): Promise<void> {
  try {
    await copyText(props.file.path)
    toast.push({ tone: "success", title: t("chat.pathCopied"), description: props.file.path })
  } catch {
    toast.push({ tone: "danger", title: t("chat.copyFailed") })
  }
}

/** Edits the file where it is shown: the same scroll, the caret on the first line in view. */
async function startEditing(): Promise<void> {
  const top = scroller.value?.scrollTop ?? 0
  const rows = [...(code.value?.children ?? [])] as HTMLElement[]
  const first = rows.find((row) => row.offsetTop + row.offsetHeight > top)
  files.edit(props.sessionId, props.file.path)
  await nextTick()
  if (scroller.value !== undefined) scroller.value.scrollTop = top
  editor.value?.focusLine(Number(first?.dataset.n ?? 1))
}

async function save(force = false): Promise<void> {
  if (!dirty.value || props.file.saving) return
  try {
    await files.save(props.sessionId, props.file.path, force)
  } catch (cause) {
    if (!props.file.stale) toast.push({ tone: "danger", title: t("files.saveFailed"), description: failure(cause) })
  }
}

/** Leaves editing, asking first when there are unsaved changes. */
async function stopEditing(): Promise<void> {
  if (dirty.value && !await confirm.value?.open(t("files.discardConfirm", { name: label.value }), { confirmLabel: t("files.discard") })) return
  if (dirty.value) await files.discard(props.sessionId, props.file.path)
  else files.finish(props.sessionId, props.file.path)
}

async function discardChanges(): Promise<void> {
  if (!await confirm.value?.open(t("files.discardConfirm", { name: label.value }), { confirmLabel: t("files.discard") })) return
  await files.discard(props.sessionId, props.file.path)
}

/** The selected lines of the file, and where to offer adding them to the message. */
const selected = ref<{ line: number; endLine: number; text: string; top: number }>()

function rowBottom(line: number): number | undefined {
  const row = scroller.value?.querySelector<HTMLElement>(`.file-line[data-n="${line}"]`)
  return row === null || row === undefined ? undefined : row.offsetTop + row.offsetHeight
}

function rowOf(node: Node | null): number | undefined {
  const element = node instanceof Element ? node : node?.parentElement
  const row = element?.closest<HTMLElement>(".file-line")
  return row?.dataset.n === undefined ? undefined : Number(row.dataset.n)
}

/** Reads the selection: the editor's, or the page's within the shown lines. */
function readSelection(): void {
  let found: { line: number; endLine: number; text: string } | undefined
  if (editing.value) {
    found = editor.value?.selection()
  } else {
    const selection = document.getSelection()
    const container = code.value
    if (selection !== null && !selection.isCollapsed && container !== undefined && selection.rangeCount > 0) {
      const range = selection.getRangeAt(0)
      if (container.contains(range.commonAncestorContainer)) {
        const line = rowOf(range.startContainer) ?? 1
        const endLine = rowOf(range.endContainer) ?? lines.value.length
        const value = selection.toString()
        if (value.trim() !== "") found = { line, endLine: Math.max(line, range.endOffset === 0 && range.endContainer !== range.startContainer ? endLine - 1 : endLine), text: value }
      }
    }
  }
  const top = found === undefined ? undefined : rowBottom(found.endLine)
  selected.value = found === undefined || top === undefined ? undefined : { ...found, top }
}

function addToMessage(): void {
  const found = selected.value
  if (found === undefined) return
  ui.requestComposerInsert(props.sessionId, selectionQuote({ path: props.file.path, label: label.value, line: found.line, endLine: found.endLine, text: found.text, lang: fileLanguage(props.file.path) }))
  selected.value = undefined
  if (editing.value) editor.value?.collapse()
  else document.getSelection()?.removeAllRanges()
}

function onSelectionChange(): void {
  if (!editing.value) readSelection()
}

onMounted(() => document.addEventListener("selectionchange", onSelectionChange))
onBeforeUnmount(() => document.removeEventListener("selectionchange", onSelectionChange))
</script>

<template>
  <div class="file-viewer">
    <ConfirmDialog ref="confirm" />
    <div class="file-toolbar">
      <span class="file-path" :title="file.path"><span>{{ label }}</span></span>
      <div class="panel-tab-actions">
        <template v-if="editing">
          <button v-if="text !== undefined" type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('files.wrap')" :aria-pressed="files.wrap" v-tooltip="t('files.wrap')" @click="files.wrap = !files.wrap">
            <WrapText aria-hidden="true" />
          </button>
          <button type="button" class="ui-button" data-variant="primary" data-size="sm" :disabled="!dirty || file.saving" v-tooltip="t('files.saveShortcut', { key: saveKey })" @click="save()">
            <Check aria-hidden="true" />{{ t(file.saving ? "files.saving" : "files.save") }}
          </button>
          <button type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('files.stopEditing')" v-tooltip="t('files.stopEditing')" @click="stopEditing">
            <X aria-hidden="true" />
          </button>
        </template>
        <template v-else>
          <button v-if="previewable" type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="mode === 'preview' ? t('files.source') : t('files.preview')" v-tooltip="mode === 'preview' ? t('files.source') : t('files.preview')" @click="mode = mode === 'preview' ? 'source' : 'preview'">
            <Code v-if="mode === 'preview'" aria-hidden="true" />
            <Eye v-else aria-hidden="true" />
          </button>
          <button v-if="text !== undefined && mode === 'source'" type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('files.wrap')" :aria-pressed="files.wrap" v-tooltip="t('files.wrap')" @click="files.wrap = !files.wrap">
            <WrapText aria-hidden="true" />
          </button>
          <button v-if="editable" type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('files.edit')" v-tooltip="t('files.edit')" @click="startEditing">
            <Pencil aria-hidden="true" />
          </button>
          <button type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('files.reload')" v-tooltip="t('files.reload')" @click="files.reload(sessionId, file.path)">
            <RefreshCw aria-hidden="true" />
          </button>
          <template v-if="canChooseWorkspace">
            <button type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('files.openExternal')" v-tooltip="t('files.openExternal')" @click="openExternally(false)">
              <ExternalLink aria-hidden="true" />
            </button>
            <button type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('files.reveal')" v-tooltip="t('files.reveal')" @click="openExternally(true)">
              <FolderOpen aria-hidden="true" />
            </button>
          </template>
        </template>
        <button type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('files.copyPath')" v-tooltip="t('files.copyPath')" @click="copyPath">
          <Copy aria-hidden="true" />
        </button>
      </div>
    </div>
    <p v-if="file.error" class="ui-banner file-banner" data-tone="danger" role="alert">{{ file.error }}</p>
    <div v-if="file.stale && dirty" class="ui-banner file-banner file-conflict" data-tone="warning" role="alert">
      <span>{{ t("files.changedOnDisk") }}</span>
      <span class="file-conflict-actions">
        <button type="button" class="ui-button" data-variant="secondary" data-size="sm" @click="discardChanges">{{ t("files.discard") }}</button>
        <button type="button" class="ui-button" data-variant="secondary" data-size="sm" :disabled="file.saving" @click="save(true)">{{ t("files.overwrite") }}</button>
      </span>
    </div>
    <p v-if="view?.kind === 'text' && view.truncated" class="ui-banner file-banner" data-tone="warning">{{ t("files.truncated") }}</p>
    <p v-if="view?.kind === 'text' && view.lossy" class="ui-banner file-banner" data-tone="warning">{{ t("files.lossy") }}</p>
    <div ref="scroller" class="file-body" @mouseup="!editing && readSelection()">
      <div v-if="view?.kind === 'image'" class="file-image"><img :src="`data:${view.mime};base64,${view.data}`" :alt="label" /></div>
      <p v-else-if="view?.kind === 'binary'" class="file-empty">{{ t("files.binary") }}</p>
      <FileEditor v-else-if="editing" ref="editor" :text="editorText" :lang="fileLanguage(file.path)" :wrap="files.wrap" @update:text="files.setDraft(sessionId, file.path, $event)" @save="save()" @select="readSelection" />
      <div v-else-if="mode === 'preview' && markdown && text !== undefined" class="file-preview"><ChatMarkdown :content="text" :final="true" :message-id="`file:${file.path}`" /></div>
      <div v-else-if="mode === 'preview' && text !== undefined" class="file-image"><img :src="svgSource" :alt="label" /></div>
      <div v-else-if="text !== undefined" ref="code" class="file-code" :data-wrap="files.wrap" :data-long="lines.length > 2000 || undefined" :style="{ '--gutter': `${String(lines.length).length}ch` }" v-html="html" />
      <button v-if="selected && files.supports(sessionId, 'session.files.read')" type="button" class="ui-button file-add-selection" data-variant="secondary" data-size="sm" :style="{ top: `${selected.top + 4}px` }" @mousedown.prevent @click="addToMessage">
        <MessageSquarePlus aria-hidden="true" />{{ t("files.addToMessage") }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.file-viewer { display: flex; min-width: 0; min-height: 0; flex-direction: column; }
.file-toolbar { display: flex; min-width: 0; height: 36px; flex: none; align-items: center; gap: var(--space-2); padding: 0 var(--space-2) 0 var(--space-3); border-bottom: 1px solid var(--border); }
.file-path { min-width: 0; flex: 1; overflow: hidden; color: var(--muted-foreground); font-size: var(--text-xs); text-align: left; text-overflow: ellipsis; white-space: nowrap; direction: rtl; }
.file-path > span { direction: ltr; unicode-bidi: isolate; }
.file-banner { flex: none; margin: var(--space-2) var(--space-3) 0; }
.file-conflict { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.file-conflict-actions { display: inline-flex; gap: var(--space-2); }
.file-body { position: relative; min-height: 0; flex: 1; overflow: auto; }
.file-image { display: grid; min-height: 100%; place-items: center; padding: var(--space-4); }
.file-image > img { max-width: 100%; height: auto; }
.file-empty { display: grid; min-height: 100%; place-items: center; margin: 0; color: var(--muted-foreground); font-size: var(--text-sm); }
.file-preview { padding: var(--space-3) var(--space-4); }
.file-add-selection { position: absolute; z-index: 2; right: var(--space-3); box-shadow: var(--shadow-sm); }
</style>
