<script setup lang="ts">
import { computed, nextTick, provide, ref, shallowRef, watch } from "vue"
import { useI18n } from "vue-i18n"
import { Code, Copy, ExternalLink, Eye, FolderOpen, RefreshCw, WrapText } from "lucide-vue-next"
import { codeToTokens } from "../../vendor/shiki-web"
import { copyText } from "../../clipboard"
import { canChooseWorkspace, openWorkspacePath } from "../../desktop"
import { useFilesStore, type OpenFile } from "../../stores/files"
import { useToastStore } from "../../stores/toast"
import ChatMarkdown from "../chat/ChatMarkdown.vue"
import { LINK_CONTEXT } from "../chat/link-context"
import { fileLanguage } from "./file-language"

/** One open file of the detail panel's file view: its numbered lines, a Markdown or SVG preview, or its image. */
const props = defineProps<{ sessionId: string; file: OpenFile }>()
const { t } = useI18n()
const files = useFilesStore()
const toast = useToastStore()
const code = ref<HTMLElement>()
const scroller = ref<HTMLElement>()
const view = computed(() => props.file.view)
const text = computed(() => view.value?.kind === "text" ? view.value.text : undefined)
const label = computed(() => view.value?.relativePath ?? props.file.path)
const markdown = computed(() => /\.(md|markdown|mdx)$/i.test(props.file.path))
const previewable = computed(() => text.value !== undefined && (markdown.value || /\.svg$/i.test(props.file.path)))
const mode = ref<"preview" | "source">(previewable.value && props.file.target === undefined ? "preview" : "source")
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

async function openExternally(reveal: boolean): Promise<void> {
  try {
    await openWorkspacePath(props.file.path, reveal)
  } catch (cause) {
    toast.push({ tone: "danger", title: t("chat.openFailed"), description: cause instanceof Error ? cause.message : String(cause) })
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
</script>

<template>
  <div class="file-viewer">
    <div class="file-toolbar">
      <span class="file-path" :title="file.path"><span>{{ label }}</span></span>
      <div class="panel-tab-actions">
        <button v-if="previewable" type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="mode === 'preview' ? t('files.source') : t('files.preview')" v-tooltip="mode === 'preview' ? t('files.source') : t('files.preview')" @click="mode = mode === 'preview' ? 'source' : 'preview'">
          <Code v-if="mode === 'preview'" aria-hidden="true" />
          <Eye v-else aria-hidden="true" />
        </button>
        <button v-if="text !== undefined && mode === 'source'" type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('files.wrap')" :aria-pressed="files.wrap" v-tooltip="t('files.wrap')" @click="files.wrap = !files.wrap">
          <WrapText aria-hidden="true" />
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
        <button type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('files.copyPath')" v-tooltip="t('files.copyPath')" @click="copyPath">
          <Copy aria-hidden="true" />
        </button>
      </div>
    </div>
    <p v-if="file.error" class="ui-banner file-banner" data-tone="danger" role="alert">{{ file.error }}</p>
    <p v-if="view?.kind === 'text' && view.truncated" class="ui-banner file-banner" data-tone="warning">{{ t("files.truncated") }}</p>
    <p v-if="view?.kind === 'text' && view.lossy" class="ui-banner file-banner" data-tone="warning">{{ t("files.lossy") }}</p>
    <div ref="scroller" class="file-body">
      <div v-if="view?.kind === 'image'" class="file-image"><img :src="`data:${view.mime};base64,${view.data}`" :alt="label" /></div>
      <p v-else-if="view?.kind === 'binary'" class="file-empty">{{ t("files.binary") }}</p>
      <div v-else-if="mode === 'preview' && markdown && text !== undefined" class="file-preview"><ChatMarkdown :content="text" :final="true" :message-id="`file:${file.path}`" /></div>
      <div v-else-if="mode === 'preview' && text !== undefined" class="file-image"><img :src="svgSource" :alt="label" /></div>
      <div v-else-if="text !== undefined" ref="code" class="file-code" :data-wrap="files.wrap" :data-long="lines.length > 2000 || undefined" :style="{ '--gutter': `${String(lines.length).length}ch` }" v-html="html" />
    </div>
  </div>
</template>

<style scoped>
.file-viewer { display: flex; min-width: 0; min-height: 0; flex-direction: column; }
.file-toolbar { display: flex; min-width: 0; height: 36px; flex: none; align-items: center; gap: var(--space-2); padding: 0 var(--space-2) 0 var(--space-3); border-bottom: 1px solid var(--border); }
.file-path { min-width: 0; flex: 1; overflow: hidden; color: var(--muted-foreground); font-size: var(--text-xs); text-align: left; text-overflow: ellipsis; white-space: nowrap; direction: rtl; }
.file-path > span { direction: ltr; unicode-bidi: isolate; }
.file-banner { flex: none; margin: var(--space-2) var(--space-3) 0; }
.file-body { position: relative; min-height: 0; flex: 1; overflow: auto; }
.file-image { display: grid; min-height: 100%; place-items: center; padding: var(--space-4); }
.file-image > img { max-width: 100%; height: auto; }
.file-empty { display: grid; min-height: 100%; place-items: center; margin: 0; color: var(--muted-foreground); font-size: var(--text-sm); }
.file-preview { padding: var(--space-3) var(--space-4); }
</style>
