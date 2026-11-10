<script setup lang="ts">
import { ref } from "vue"
import { useI18n } from "vue-i18n"
import { Copy, ExternalLink, FileText, FolderOpen, MessageSquarePlus, SquareTerminal } from "lucide-vue-next"
import RowActionMenu from "../../../components/RowActionMenu.vue"
import { copyText } from "../../../clipboard"
import { canChooseWorkspace, openWorkspacePath } from "../../../desktop"
import { fileLink } from "../../../mentions"
import { useFilesStore } from "../../../stores/files"
import { usePanesStore } from "../../../stores/panes"
import { useTerminalsStore } from "../../../stores/terminals"
import { useToastStore } from "../../../stores/toast"
import { useUiStore } from "../../../stores/ui"

/** A file or folder of the Session's workspace: its absolute `path` and the `label` it is shown by. */
export type FileTarget = { path: string; label: string; dir: boolean }

/**
 * What a right click on a file or folder offers, as Claude Code's: open it,
 * add it to the message, copy its path, a terminal in the folder, and Finder
 * in the desktop app.
 */
const props = defineProps<{ sessionId: string }>()
const { t } = useI18n()
const files = useFilesStore()
const panes = usePanesStore()
const terminals = useTerminalsStore()
const toast = useToastStore()
const ui = useUiStore()
const menu = ref<InstanceType<typeof RowActionMenu>>()
const target = ref<FileTarget>()

function open(event: MouseEvent, file: FileTarget): void {
  target.value = file
  void menu.value?.open(event)
}

function failed(cause: unknown): void {
  toast.push({ tone: "danger", title: t("chat.openFailed"), description: cause instanceof Error ? cause.message : String(cause) })
}

async function show(path: string): Promise<void> {
  try {
    await files.open(props.sessionId, path, undefined, true)
  } catch (cause) {
    failed(cause)
  }
}

async function copy(text: string): Promise<void> {
  try {
    await copyText(text)
    toast.push({ tone: "success", title: t("chat.pathCopied"), description: text })
  } catch {
    toast.push({ tone: "danger", title: t("chat.copyFailed") })
  }
}

async function openTerminal(path: string): Promise<void> {
  try {
    await terminals.open(props.sessionId, path)
    panes.show(props.sessionId, "terminal")
  } catch (cause) {
    toast.push({ tone: "danger", title: t("files.terminalFailed"), description: cause instanceof Error ? cause.message : String(cause) })
  }
}

async function external(path: string, reveal: boolean): Promise<void> {
  try {
    await openWorkspacePath(path, reveal)
  } catch (cause) {
    failed(cause)
  }
}

defineExpose({ open })
</script>

<template>
  <RowActionMenu ref="menu" :label="t('files.actions', { name: target?.label ?? '' })">
    <template #trigger />
    <template v-if="target">
      <button v-if="!target.dir" type="button" class="ui-menu-item" @click="show(target.path)"><FileText aria-hidden="true" />{{ t("files.open") }}</button>
      <button type="button" class="ui-menu-item" @click="ui.requestComposerInsert(sessionId, fileLink(`@${target.label}`, target.path))"><MessageSquarePlus aria-hidden="true" />{{ t("files.addToMessage") }}</button>
      <button v-if="target.dir && terminals.available" type="button" class="ui-menu-item" @click="openTerminal(target.path)"><SquareTerminal aria-hidden="true" />{{ t("files.openInTerminal") }}</button>
      <hr class="ui-menu-separator">
      <button type="button" class="ui-menu-item" @click="copy(target.path)"><Copy aria-hidden="true" />{{ t("files.copyPath") }}</button>
      <button v-if="target.label !== target.path" type="button" class="ui-menu-item" @click="copy(target.label)"><Copy aria-hidden="true" />{{ t("files.copyRelativePath") }}</button>
      <template v-if="canChooseWorkspace">
        <hr class="ui-menu-separator">
        <button v-if="!target.dir" type="button" class="ui-menu-item" @click="external(target.path, false)"><ExternalLink aria-hidden="true" />{{ t("files.openExternal") }}</button>
        <button type="button" class="ui-menu-item" @click="external(target.path, true)"><FolderOpen aria-hidden="true" />{{ t("files.reveal") }}</button>
      </template>
    </template>
  </RowActionMenu>
</template>
