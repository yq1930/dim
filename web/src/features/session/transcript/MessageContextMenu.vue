<script setup lang="ts">
import { ref } from "vue"
import { useI18n } from "vue-i18n"
import { Copy, GitBranch, Link, Quote, TextCursorInput, TextQuote } from "lucide-vue-next"
import RowActionMenu from "../../../components/RowActionMenu.vue"
import { copyText } from "../../../clipboard"
import type { TimelineRow } from "../../../projection"
import { useToastStore } from "../../../stores/toast"

type MessageRow = Extract<TimelineRow, { kind: "user" | "assistant" }>

/**
 * What a right click on a message offers, as Claude Code's: the selected text,
 * the whole message as it was written, and the link under the pointer.
 */
defineProps<{ quotable: boolean; branchable: boolean }>()
const emit = defineEmits<{ quote: [text: string]; branch: [text: string, cut: number] }>()
const { t } = useI18n()
const toast = useToastStore()
const menu = ref<InstanceType<typeof RowActionMenu>>()
const target = ref<{ row: MessageRow; selection: string; link?: string }>()

function open(event: MouseEvent, row: MessageRow): void {
  const message = event.currentTarget as Element
  const selection = window.getSelection()
  const selected = selection !== null && !selection.isCollapsed && message.contains(selection.anchorNode) && message.contains(selection.focusNode) ? selection.toString().trim() : ""
  const link = event.target instanceof Element ? event.target.closest("a[href]")?.getAttribute("href") ?? undefined : undefined
  target.value = { row, selection: selected, link }
  void menu.value?.open(event)
}

async function copy(text: string): Promise<void> {
  try {
    await copyText(text)
    toast.push({ tone: "success", title: t("timeline.copied") })
  } catch {
    toast.push({ tone: "danger", title: t("chat.copyFailed") })
  }
}

defineExpose({ open })
</script>

<template>
  <RowActionMenu ref="menu" :label="t('message.actions')">
    <template #trigger />
    <template v-if="target">
      <template v-if="target.selection">
        <button type="button" class="ui-menu-item" @click="copy(target.selection)"><TextCursorInput aria-hidden="true" />{{ t("message.copySelection") }}</button>
        <button v-if="quotable" type="button" class="ui-menu-item" @click="emit('quote', target.selection)"><TextQuote aria-hidden="true" />{{ t("message.quoteSelection") }}</button>
        <hr class="ui-menu-separator">
      </template>
      <button type="button" class="ui-menu-item" @click="copy(target.row.text)"><Copy aria-hidden="true" />{{ t(target.row.kind === "user" ? "message.copy" : "message.copyMarkdown") }}</button>
      <button v-if="quotable" type="button" class="ui-menu-item" @click="emit('quote', target.row.text)"><Quote aria-hidden="true" />{{ t("message.quote") }}</button>
      <button v-if="branchable && target.row.kind === 'user'" type="button" class="ui-menu-item" @click="emit('branch', target.row.text, target.row.cut)"><GitBranch aria-hidden="true" />{{ t("message.branch") }}</button>
      <template v-if="target.link">
        <hr class="ui-menu-separator">
        <button type="button" class="ui-menu-item" @click="copy(target.link)"><Link aria-hidden="true" />{{ t("message.copyLink") }}</button>
      </template>
    </template>
  </RowActionMenu>
</template>
