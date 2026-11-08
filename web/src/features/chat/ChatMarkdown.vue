<script lang="ts">
import type { Component } from "vue"

// Module scope, not per instance: a per-row cache made every row render its plain
// text first and swap in the renderer a tick later, so each session switch kept
// re-measuring the timeline and held the restore cover open.
let renderer: Component | undefined
</script>

<script setup lang="ts">
import { copyText } from "../../clipboard"
import { canChooseWorkspace, openWorkspacePath } from "../../desktop"
import { loadMarkdownRenderer } from "./markdown-renderer"
import { inject, onBeforeUnmount, onMounted, ref, shallowRef } from "vue"
import { useI18n } from "vue-i18n"
import { useFilesStore } from "../../stores/files"
import { useToastStore } from "../../stores/toast"
import { LINK_CONTEXT } from "./link-context"
import { markdownLinkLines, markdownLinkPath } from "./markdown-links"

defineProps<{
  content: string
  final: boolean
  messageId: string
}>()

const { t } = useI18n()
const toast = useToastStore()
const files = useFilesStore()
const link = inject(LINK_CONTEXT, undefined)
/** markstream sizes code from a number, not CSS: 13px on 20px lines, the 14px type scale's code size. */
const codeBlockOptions = { fontSize: 13, lineHeight: 20 }
const MarkdownRender = shallowRef(renderer)
const isDark = ref(false)
let observer: MutationObserver | undefined

function readTheme(): void {
  isDark.value = document.documentElement.dataset.theme === "dark"
    || document.documentElement.classList.contains("dark")
}

onMounted(async () => {
  readTheme()
  observer = new MutationObserver(readTheme)
  observer.observe(document.documentElement, { attributes: true, attributeFilter: ["class", "data-theme"] })
  renderer ??= await loadMarkdownRenderer()
  MarkdownRender.value = renderer
})

onBeforeUnmount(() => observer?.disconnect())

/**
 * A workspace path is not a page. A file of the Session's workspace opens in the
 * detail panel's file view, at the line the link names; the desktop app opens
 * any other path, and ⌘-click shows it in Finder; a browser copies it instead of
 * letting the click leave the session.
 */
async function onLinkClick(event: MouseEvent): Promise<void> {
  const anchor = event.target instanceof Element ? event.target.closest("a[href]") : null
  const href = anchor?.getAttribute("href") ?? ""
  const path = anchor === null ? undefined : markdownLinkPath(href)
  if (path === undefined) return
  event.preventDefault()
  const context = link?.value
  const absolute = /^[/~]/.test(path) ? path : context === undefined ? undefined : `${context.base.replace(/\/+$/, "")}/${path}`
  if (absolute !== undefined && context?.filePane === true && !event.metaKey && files.readable(context.sessionId)) {
    try {
      if (await files.open(context.sessionId, absolute, markdownLinkLines(href))) return
    } catch (cause) {
      toast.push({ tone: "danger", title: t("chat.openFailed"), description: cause instanceof Error ? cause.message : String(cause) })
      return
    }
  }
  if (canChooseWorkspace && absolute !== undefined) {
    try {
      await openWorkspacePath(absolute, event.metaKey)
    } catch (cause) {
      toast.push({ tone: "danger", title: t("chat.openFailed"), description: cause instanceof Error ? cause.message : String(cause) })
    }
    return
  }
  try {
    await copyText(path)
    toast.push({ tone: "success", title: t("chat.pathCopied"), description: path })
  } catch {
    toast.push({ tone: "danger", title: t("chat.copyFailed") })
  }
}
</script>

<template>
  <component
    :is="MarkdownRender"
    v-if="MarkdownRender"
    class="chat-markdown"
    :class="{ dark: isDark }"
    :content="content"
    :final="final"
    :index-key="messageId"
    :is-dark="isDark"
    :code-block-options="codeBlockOptions"
    html-policy="escape"
    mode="chat"
    :node-virtual="false"
    @click="onLinkClick"
  />
  <div v-else class="chat-markdown chat-markdown--pending">{{ content }}</div>
</template>
