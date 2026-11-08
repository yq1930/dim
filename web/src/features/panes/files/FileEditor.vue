<script setup lang="ts">
import { computed, onBeforeUnmount, ref, shallowRef, watch } from "vue"
import { codeToTokens } from "../../../vendor/shiki-web"

/**
 * A text file being edited: a text field laid exactly over the file view's
 * numbered, highlighted lines. The field's own text is transparent, so the lines
 * underneath show it; they follow each keystroke as plain text and are
 * highlighted again once typing pauses.
 */
const props = defineProps<{ text: string; lang?: string; wrap: boolean }>()
const emit = defineEmits<{ "update:text": [text: string]; save: []; select: [] }>()
const field = ref<HTMLTextAreaElement>()
/** Each line's highlighted HTML from the last highlighting, by the line's text. */
const highlighted = shallowRef(new Map<string, string>())
const HIGHLIGHT_MAX_CHARS = 256 * 1024
const HIGHLIGHT_DELAY_MS = 250
let timer: ReturnType<typeof setTimeout> | undefined
let job = 0

const lines = computed(() => props.text.split("\n"))
const html = computed(() => {
  const known = highlighted.value
  return lines.value.map((line, index) => `<div class="file-line" data-n="${index + 1}"><span class="file-text">${known.get(line) ?? escapeHtml(line)}</span></div>`).join("")
})
/** One indent: a tab where the file indents with tabs, else its narrowest indent of spaces. */
const indent = computed(() => {
  let narrowest = 0
  for (const line of lines.value) {
    if (line.startsWith("\t")) return "\t"
    const spaces = /^( +)\S/.exec(line)?.[1]?.length ?? 0
    if (spaces > 0 && (narrowest === 0 || spaces < narrowest)) narrowest = spaces
  }
  return " ".repeat(narrowest === 0 || narrowest > 8 ? 2 : narrowest)
})

function escapeHtml(value: string): string {
  return value.replace(/[&<>"]/g, (ch) => ch === "&" ? "&amp;" : ch === "<" ? "&lt;" : ch === ">" ? "&gt;" : "&quot;")
}

async function highlight(): Promise<void> {
  const request = ++job
  const value = props.text
  if (props.lang === undefined || value.length > HIGHLIGHT_MAX_CHARS) {
    highlighted.value = new Map()
    return
  }
  try {
    const { tokens } = await codeToTokens(value, { lang: props.lang, themes: { light: "vitesse-light", dark: "vitesse-dark" }, defaultColor: false })
    if (request !== job) return
    const known = new Map<string, string>()
    value.split("\n").forEach((line, index) => {
      known.set(line, (tokens[index] ?? []).map((token) => {
        const style = Object.entries(token.htmlStyle ?? {}).map(([key, color]) => `${key}:${color}`).join(";")
        return `<span style="${escapeHtml(style)}">${escapeHtml(token.content)}</span>`
      }).join(""))
    })
    highlighted.value = known
  } catch {
    // An unbundled grammar leaves the text plain.
  }
}

watch(() => [props.text, props.lang] as const, () => {
  clearTimeout(timer)
  timer = setTimeout(() => { void highlight() }, HIGHLIGHT_DELAY_MS)
}, { immediate: true })
onBeforeUnmount(() => clearTimeout(timer))

/** Types `text` at the caret as the browser would, so undo still works. */
function insert(text: string): void {
  if (document.execCommand("insertText", false, text)) return
  const element = field.value
  if (element === undefined) return
  element.setRangeText(text, element.selectionStart, element.selectionEnd, "end")
  emit("update:text", element.value)
}

function onKey(event: KeyboardEvent): void {
  if (event.isComposing) return
  const mod = event.metaKey || event.ctrlKey
  if (mod && !event.altKey && event.key.toLowerCase() === "s") {
    event.preventDefault()
    emit("save")
    return
  }
  const element = field.value
  if (element === undefined || mod || event.altKey) return
  if (event.key === "Tab" && !event.shiftKey) {
    event.preventDefault()
    insert(indent.value)
  } else if (event.key === "Enter" && !event.shiftKey) {
    // A new line keeps the indent of the one it breaks.
    const start = element.value.lastIndexOf("\n", element.selectionStart - 1) + 1
    const leading = /^[ \t]*/.exec(element.value.slice(start, element.selectionStart))?.[0] ?? ""
    if (leading === "") return
    event.preventDefault()
    insert(`\n${leading}`)
  }
}

/** The lines the selection covers and its text; none for an empty selection. */
function selection(): { line: number; endLine: number; text: string } | undefined {
  const element = field.value
  if (element === undefined || element.selectionStart === element.selectionEnd) return undefined
  const before = element.value.slice(0, element.selectionStart)
  const text = element.value.slice(element.selectionStart, element.selectionEnd)
  const line = before.split("\n").length
  const endLine = line + text.replace(/\n$/, "").split("\n").length - 1
  return { line, endLine, text }
}

/** Puts the caret at the start of `line` without scrolling, for the view the edit replaces. */
function focusLine(line: number): void {
  const element = field.value
  if (element === undefined) return
  const offset = lines.value.slice(0, Math.max(0, line - 1)).reduce((total, text) => total + text.length + 1, 0)
  element.focus({ preventScroll: true })
  element.setSelectionRange(offset, offset)
}

/** Collapses the selection to its end, as after copying it elsewhere. */
function collapse(): void {
  const element = field.value
  if (element !== undefined) element.setSelectionRange(element.selectionEnd, element.selectionEnd)
}

defineExpose({ selection, focusLine, collapse })
</script>

<template>
  <div class="file-code file-editor" :data-wrap="wrap" :style="{ '--gutter': `${String(lines.length).length}ch` }">
    <div class="file-editor-layer" aria-hidden="true" v-html="html" />
    <textarea
      ref="field"
      class="file-editor-input"
      :value="text"
      :wrap="wrap ? 'soft' : 'off'"
      spellcheck="false"
      autocapitalize="off"
      autocomplete="off"
      @input="emit('update:text', ($event.target as HTMLTextAreaElement).value)"
      @keydown="onKey"
      @select="emit('select')"
      @keyup="emit('select')"
      @mouseup="emit('select')"
    />
  </div>
</template>

<style scoped>
.file-editor { position: relative; min-height: 100%; }
.file-editor-input { position: absolute; inset: var(--space-2) 0 var(--space-2) calc(var(--gutter) + 2 * var(--space-3)); width: auto; height: auto; margin: 0; padding: 0 var(--space-3) 0 0; overflow: hidden; border: 0; outline: none; resize: none; background: transparent; color: transparent; caret-color: var(--foreground); font: inherit; tab-size: inherit; white-space: pre; }
.file-editor[data-wrap="true"] .file-editor-input { white-space: pre-wrap; overflow-wrap: anywhere; }
.file-editor-input::selection { background: color-mix(in oklab, var(--primary) 28%, transparent); color: transparent; }
</style>
