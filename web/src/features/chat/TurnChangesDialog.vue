<script setup lang="ts">
import { computed, onBeforeUnmount, ref, useId } from "vue"
import { useI18n } from "vue-i18n"
import { FilePlus, FileText } from "lucide-vue-next"
import type { TranslationKey } from "../../i18n"
import { RpcError } from "../../rpc/client"
import type { FileChange, RewindResult, TurnChanges } from "../../rpc/protocol"
import { useToastStore } from "../../stores/toast"

/**
 * A turn's `write` and `edit` changes and their undo (docs/design/file-rewind.md):
 * the server decides each file's status; undo sends the files it shows as restorable.
 */
const props = defineProps<{
  load: (turnId: string) => Promise<TurnChanges>
  rewind: (turnId: string, paths: string[]) => Promise<RewindResult>
  /** The Session's workspace, so paths under it show relative. */
  cwd?: string
}>()
const { t } = useI18n()
const toast = useToastStore()
const id = useId()
const dialog = ref<HTMLDialogElement>()
const turnId = ref<string>()
const changes = ref<TurnChanges>()
const loading = ref(false)
const busy = ref(false)
const error = ref("")
const notice = ref("")
let sequence = 0

const STATUS_KEYS = {
  restored: "changes.status.restored",
  changed: "changes.status.changed",
  unrecorded: "changes.status.unrecorded",
} as const satisfies Record<Exclude<FileChange["status"], "restorable">, TranslationKey>

const restorable = computed(() => changes.value?.files.filter((file) => file.status === "restorable") ?? [])

function shown(path: string): string {
  const root = props.cwd?.replace(/[\\/]+$/, "")
  return root !== undefined && root !== "" && (path.startsWith(`${root}/`) || path.startsWith(`${root}\\`)) ? path.slice(root.length + 1) : path
}

async function reload(): Promise<void> {
  const turn = turnId.value
  if (turn === undefined) return
  const request = ++sequence
  try {
    const found = await props.load(turn)
    if (request === sequence) { changes.value = found; error.value = "" }
  } catch (cause) {
    if (request === sequence) error.value = cause instanceof Error ? cause.message : String(cause)
  } finally {
    if (request === sequence) loading.value = false
  }
}

function open(turn: string): void {
  turnId.value = turn
  changes.value = undefined
  error.value = ""
  notice.value = ""
  loading.value = true
  dialog.value?.showModal()
  void reload()
}

function close(): void {
  ++sequence
  turnId.value = undefined
  dialog.value?.close()
}

async function undo(): Promise<void> {
  const turn = turnId.value
  if (turn === undefined || restorable.value.length === 0) return
  busy.value = true
  notice.value = ""
  try {
    const result = await props.rewind(turn, restorable.value.map((file) => file.path))
    const count = result.restored.length + result.removed.length
    if (count > 0) toast.push({ tone: "success", title: t("timeline.filesRewound", { count }, count) })
    if (result.failed !== undefined) notice.value = t("changes.stopped", { path: shown(result.failed.path), message: result.failed.message })
    else if (count > 0 && !result.recorded) notice.value = t("changes.notRecorded")
    if (notice.value === "") { close(); return }
  } catch (cause) {
    notice.value = cause instanceof RpcError && cause.appError === "rewind_conflict" ? t("changes.conflict") : cause instanceof Error ? cause.message : String(cause)
  } finally {
    busy.value = false
  }
  await reload()
}

onBeforeUnmount(close)
defineExpose({ open })
</script>

<template>
  <dialog ref="dialog" class="ui-dialog changes-dialog" data-size="lg" :aria-labelledby="id" @cancel.prevent="close" @close="close">
    <header class="ui-dialog-header"><h2 :id="id" class="ui-dialog-title">{{ t("changes.title") }}</h2></header>
    <div class="changes-body">
      <p v-if="loading" role="status">{{ t("common.loading") }}</p>
      <p v-else-if="error" class="ui-banner" data-tone="danger" role="alert">{{ error }}</p>
      <template v-else-if="changes">
        <p v-if="notice" class="ui-banner" data-tone="warning" role="alert">{{ notice }}</p>
        <p v-if="!changes.files.length" role="status">{{ t("changes.empty") }}</p>
        <ul v-else class="changes-files">
          <li v-for="file in changes.files" :key="file.path" class="changes-file">
            <FilePlus v-if="file.change === 'created'" aria-hidden="true" />
            <FileText v-else aria-hidden="true" />
            <span class="changes-path" :title="file.path">{{ shown(file.path) }}</span>
            <span v-if="file.change === 'created'" class="ui-badge" data-size="sm">{{ t("changes.created") }}</span>
            <span v-if="file.status !== 'restorable'" class="ui-badge" data-size="sm" :data-tone="file.status === 'restored' ? 'success' : 'warning'">{{ t(STATUS_KEYS[file.status]) }}</span>
          </li>
        </ul>
        <div v-if="changes.commands.length" class="changes-commands">
          <p>{{ t("changes.commands") }}</p>
          <code v-for="(command, index) in changes.commands" :key="index">{{ command.command }}</code>
        </div>
        <p v-if="!changes.ended" class="changes-hint" role="status">{{ t("changes.running") }}</p>
      </template>
    </div>
    <footer class="ui-dialog-footer">
      <button type="button" class="ui-button" data-variant="secondary" @click="close">{{ t("common.close") }}</button>
      <button
        v-if="restorable.length"
        type="button"
        class="ui-button"
        data-variant="danger"
        :disabled="busy || changes?.ended !== true"
        @click="undo"
      >
        {{ t("changes.undo", { count: restorable.length }, restorable.length) }}
      </button>
    </footer>
  </dialog>
</template>

<style scoped>
.changes-body { display: grid; gap: var(--space-3); max-height: 60dvh; overflow: auto; padding: var(--space-3) var(--space-4); }
.changes-body > p { margin: 0; color: var(--muted-foreground); }
.changes-files { display: grid; gap: var(--space-1); margin: 0; padding: 0; list-style: none; }
.changes-file { display: flex; min-width: 0; align-items: center; gap: var(--space-2); font-size: var(--text-md); }
.changes-file > svg { flex: none; width: 14px; height: 14px; color: var(--muted-foreground); }
.changes-path { min-width: 0; flex: 1; overflow: hidden; font-family: var(--font-mono); font-size: var(--text-sm); text-overflow: ellipsis; white-space: nowrap; }
.changes-file .ui-badge { flex: none; }
.changes-commands { display: grid; gap: var(--space-1); color: var(--muted-foreground); font-size: var(--text-sm); }
.changes-commands p { margin: 0; }
.changes-commands code { overflow: hidden; font-family: var(--font-mono); font-size: var(--text-xs); text-overflow: ellipsis; white-space: nowrap; }
.changes-body > .changes-hint { font-size: var(--text-sm); }
</style>
