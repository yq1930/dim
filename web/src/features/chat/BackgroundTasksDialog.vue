<script setup lang="ts">
import { computed, onBeforeUnmount, ref, useId, watch } from "vue"
import { useI18n } from "vue-i18n"
import { RefreshCw, Square, Terminal, X } from "lucide-vue-next"
import ConfirmDialog from "../../components/ConfirmDialog.vue"
import { RpcError } from "../../rpc/client"
import { MAIN_BRANCH } from "../../rpc/protocol"
import { useChatStore, type SessionTask } from "../../stores/chat"
import { useConnectionStore } from "../../stores/connection"

const props = defineProps<{ sessionId: string }>()
const { t } = useI18n()
const chat = useChatStore()
const connection = useConnectionStore()
const id = useId()
const dialog = ref<HTMLDialogElement>()
const confirm = ref<InstanceType<typeof ConfirmDialog>>()
const opened = ref(false)
const selectedKey = ref("")
const limit = ref(50)
const output = ref("")
const outputError = ref("")
const stopError = ref("")
const unavailable = ref(false)
const reading = ref(false)
const stopping = ref(false)
const view = computed(() => chat.views[props.sessionId])
const tasks = computed(() => view.value?.backgroundTasks ?? [])
const selected = computed(() => tasks.value.find((task) => key(task) === selectedKey.value))
const detailOutput = computed(() => selected.value?.finish ? selected.value.finish.result.text : output.value)
const branches = computed(() => Object.entries(view.value?.branches ?? {}))
const incomplete = computed(() => branches.value.filter(([, branch]) => branch.status === "failed"))
const loadingHistory = computed(() => view.value?.status === "loading" || branches.value.some(([, branch]) => branch.status === "loading"))
const canRead = computed(() => view.value?.methods.includes("session.exec.output") === true)
const canStop = computed(() => connection.supports("session.exec.stop") || view.value?.methods.includes("session.exec.stop") === true)
let generation = 0
let pending = false
let timer: ReturnType<typeof setTimeout> | undefined

function key(task: SessionTask): string { return JSON.stringify([task.branchId, task.taskId]) }
function branchName(branchId: string): string {
  return branchId === MAIN_BRANCH ? t("tasks.main") : view.value?.lines.find((line) => line.id === branchId)?.purpose || branchId
}
function invalidate(): void { ++generation; clearTimeout(timer) }
function active(): boolean {
  return opened.value && !!selected.value && !selected.value.finish && canRead.value && connection.phase === "connected" && !unavailable.value && outputError.value === ""
}
async function poll(): Promise<void> {
  if (pending || !active()) return
  const task = selected.value!
  const sessionId = props.sessionId
  const request = generation
  pending = true
  reading.value = true
  try {
    const text = await chat.taskOutput(sessionId, task.branchId, task.taskId)
    if (request === generation) output.value = text
  } catch (cause) {
    if (request === generation) {
      if (cause instanceof RpcError && (cause.appError === "exec_task_unknown" || cause.appError === "unknown_method" || cause.code === -32601)) unavailable.value = true
      else outputError.value = cause instanceof Error ? cause.message : String(cause)
    }
  } finally {
    pending = false
    reading.value = false
    if (active()) timer = setTimeout(() => void poll(), request === generation ? 1000 : 0)
  }
}

watch(tasks, (items) => {
  if (!items.some((task) => key(task) === selectedKey.value)) selectedKey.value = items[0] ? key(items[0]) : ""
}, { immediate: true })
watch([opened, selectedKey, () => props.sessionId, () => selected.value?.finish, () => connection.phase], (next, previous) => {
  invalidate()
  if (next[1] !== previous?.[1] || next[2] !== previous?.[2]) { output.value = ""; stopError.value = "" }
  outputError.value = ""
  unavailable.value = false
  void poll()
})

function retry(): void {
  invalidate()
  outputError.value = ""
  unavailable.value = false
  void poll()
}
function retryHistory(): void {
  for (const [branchId] of incomplete.value) chat.retryLine(props.sessionId, branchId)
}
function open(): void {
  limit.value = 50
  opened.value = true
  dialog.value?.showModal()
  retryHistory()
}
function close(): void {
  invalidate()
  opened.value = false
  dialog.value?.close()
}
async function stop(): Promise<void> {
  const task = selected.value
  if (!task || task.finish || stopping.value || connection.phase !== "connected") return
  const sessionId = props.sessionId
  const request = generation
  if (!await confirm.value?.open(t("tasks.stopConfirm", { command: task.label }), { confirmLabel: t("tasks.stop") })) return
  if (request !== generation || selected.value?.finish || connection.phase !== "connected") return
  stopping.value = true
  stopError.value = ""
  try { await chat.stopTask(sessionId, task.branchId, task.taskId) }
  catch (cause) { if (request === generation) stopError.value = cause instanceof Error ? cause.message : String(cause) }
  finally { stopping.value = false }
}
onBeforeUnmount(close)
defineExpose({ open })
</script>

<template>
  <dialog ref="dialog" class="ui-dialog tasks-dialog" :aria-labelledby="id" @cancel.prevent="close">
    <header class="ui-dialog-header">
      <h2 :id="id" class="ui-dialog-title">{{ t("tasks.title") }}</h2>
      <button type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('common.close')" @click="close"><X aria-hidden="true" /></button>
    </header>
    <template v-if="opened">
      <p v-if="connection.phase !== 'connected'" class="ui-banner" role="status">{{ t("tasks.offline") }}</p>
      <p v-if="loadingHistory" role="status">{{ t("tasks.loading") }}</p>
      <div v-if="incomplete.length" class="ui-banner" data-tone="warning" role="alert"><span>{{ t("tasks.incomplete") }}</span><button type="button" class="ui-button" data-variant="ghost" data-size="sm" :disabled="connection.phase !== 'connected'" @click="retryHistory">{{ t("tasks.retry") }}</button></div>
      <p v-if="!tasks.length && !loadingHistory && !incomplete.length" class="tasks-empty">{{ t("tasks.empty") }}</p>
      <div v-if="tasks.length" class="tasks-body">
        <nav class="tasks-list" :aria-label="t('tasks.title')">
          <button v-for="task in tasks.slice(0, limit)" :key="key(task)" type="button" class="ui-menu-item task-item" :aria-pressed="key(task) === selectedKey" @click="selectedKey = key(task)">
            <Terminal aria-hidden="true" />
            <span class="task-name"><strong :title="task.label">{{ task.label }}</strong><small>{{ branchName(task.branchId) }}</small></span>
            <span class="ui-badge" :data-tone="task.finish?.status === 'failed' ? 'danger' : task.finish ? undefined : 'primary'">{{ t(`tasks.${task.finish?.status ?? 'running'}`) }}</span>
          </button>
          <button v-if="tasks.length > limit" type="button" class="ui-button" data-variant="ghost" data-size="sm" @click="limit += 50">{{ t("sidebar.showMore") }}</button>
        </nav>
        <section v-if="selected" class="task-detail" :aria-label="t('tasks.output')">
          <div class="task-toolbar"><strong>{{ branchName(selected.branchId) }}</strong><span>{{ t(`tasks.${selected.finish?.status ?? 'running'}`) }}</span></div>
          <code class="task-command">{{ selected.label }}</code>
          <p class="task-hint">{{ t(selected.finish ? "tasks.finalOutput" : "tasks.preview") }}</p>
          <p v-if="!selected.finish && (!canRead || unavailable)" role="status">{{ t("tasks.unavailable") }}</p>
          <p v-if="outputError" class="ui-banner" data-tone="danger" role="alert">{{ outputError }}</p>
          <pre v-if="detailOutput" class="task-output" tabindex="0" :aria-label="t('tasks.output')">{{ detailOutput }}</pre>
          <p v-else role="status">{{ t(reading && !selected.finish ? "common.loading" : "tasks.noOutput") }}</p>
          <p v-if="stopError" class="ui-banner" data-tone="danger" role="alert">{{ stopError }}</p>
          <footer class="ui-dialog-footer">
            <button v-if="!selected.finish && canRead" type="button" class="ui-button" data-variant="secondary" data-size="sm" :disabled="reading || connection.phase !== 'connected'" @click="retry"><RefreshCw aria-hidden="true" />{{ t("tasks.refresh") }}</button>
            <button v-if="!selected.finish && canStop" type="button" class="ui-button" data-variant="danger" data-size="sm" :disabled="stopping || connection.phase !== 'connected'" @click="stop"><Square aria-hidden="true" />{{ t(stopping ? "tasks.stopping" : "tasks.stop") }}</button>
          </footer>
        </section>
      </div>
    </template>
  </dialog>
  <ConfirmDialog ref="confirm" />
</template>

<style scoped>
.tasks-dialog { width: min(960px, calc(100vw - var(--space-8))); max-height: calc(100dvh - var(--space-8)); overflow: auto; }
.tasks-body { display: grid; grid-template-columns: minmax(220px, 1fr) minmax(0, 1.5fr); gap: var(--space-4); margin-top: var(--space-4); }
.tasks-list { max-height: 60dvh; overflow: auto; }
.task-item { align-items: flex-start; gap: var(--space-2); padding-block: var(--space-2); }
.task-item[aria-pressed="true"] { background: var(--hover); }
.task-name { min-width: 0; flex: 1; }
.task-name strong, .task-name small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.task-name small, .task-hint, .tasks-empty { color: var(--fg-2); }
.task-name small, .task-hint { font-size: var(--text-xs); }
.task-item .ui-badge { flex: none; }
.task-detail { min-width: 0; border-left: 1px solid var(--border); padding-left: var(--space-4); }
.task-toolbar { display: flex; justify-content: space-between; gap: var(--space-2); margin-bottom: var(--space-2); }
.task-toolbar strong { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.task-command { display: block; white-space: pre-wrap; overflow-wrap: anywhere; }
.task-output { max-height: 40dvh; min-height: 100px; overflow: auto; padding: var(--space-3); background: var(--muted); border-radius: var(--radius-md); font: var(--text-sm)/1.5 var(--font-mono); white-space: pre-wrap; overflow-wrap: anywhere; }
@media (max-width: 720px) {
  .tasks-body { grid-template-columns: minmax(0, 1fr); }
  .tasks-list { max-height: 24dvh; }
  .task-detail { border-left: 0; border-top: 1px solid var(--border); padding: var(--space-3) 0 0; }
}
</style>
