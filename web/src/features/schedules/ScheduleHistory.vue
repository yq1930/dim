<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, useId } from "vue"
import { RouterLink } from "vue-router"
import { useI18n } from "vue-i18n"
import { RefreshCw, X } from "lucide-vue-next"
import { readScheduleRuns, type ScheduleJob, type ScheduleRun } from "../../rpc/schedule"
import { useConnectionStore } from "../../stores/connection"
import { formatElapsedSeconds } from "../session/transcript/timeline-time"
import { formatWhen } from "./schedule-form"
import { RUN_TONE, runDetail } from "./run-state"

/** A scheduled task's runs, newest first, each with the Session it ran in. */
const { t, locale } = useI18n()
const connection = useConnectionStore()
const id = useId()
const dialog = ref<HTMLDialogElement>()
const job = ref<ScheduleJob>()
const runs = ref<ScheduleRun[]>([])
const loading = ref(false)
const error = ref("")
let generation = 0

async function load(): Promise<void> {
  const current = job.value
  if (current === undefined) return
  const request = ++generation
  loading.value = true
  error.value = ""
  try {
    const found = await connection.client.call("schedule.history", { job_id: current.id, limit: 50 }, readScheduleRuns)
    if (request === generation) runs.value = found
  } catch (cause) {
    if (request === generation) error.value = cause instanceof Error ? cause.message : String(cause)
  } finally {
    if (request === generation) loading.value = false
  }
}

async function open(target: ScheduleJob): Promise<void> {
  job.value = target
  runs.value = []
  dialog.value?.showModal()
  await nextTick()
  await load()
}

function close(): void {
  ++generation
  dialog.value?.close()
}

function duration(run: ScheduleRun): string | undefined {
  return run.endedAt === undefined ? undefined : formatElapsedSeconds(new Date(run.endedAt).getTime() - new Date(run.createdAt).getTime())
}

onBeforeUnmount(close)
defineExpose({ open })
</script>

<template>
  <dialog ref="dialog" class="ui-dialog schedule-history" data-size="lg" :aria-labelledby="id" @cancel.prevent="close">
    <header class="ui-dialog-header">
      <h2 :id="id" class="ui-dialog-title">{{ t("schedules.historyOf", { name: job?.name ?? "" }) }}</h2>
      <button type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :disabled="loading" :aria-label="t('schedules.refresh')" v-tooltip="t('schedules.refresh')" @click="load"><RefreshCw aria-hidden="true" /></button>
      <button type="button" class="ui-icon-button ui-dialog-close" data-variant="ghost" data-size="sm" :aria-label="t('common.close')" @click="close"><X aria-hidden="true" /></button>
    </header>
    <p v-if="error" class="ui-banner" data-tone="danger" role="alert">{{ error }}</p>
    <p v-else-if="!loading && runs.length === 0" class="history-empty">{{ t("schedules.noRuns") }}</p>
    <ol v-else class="history-list">
      <li v-for="run in runs" :key="run.runId" class="history-run">
        <span class="ui-badge" :data-tone="RUN_TONE[run.state]">{{ t(`schedules.states.${run.state}`) }}</span>
        <span class="history-when">{{ run.window === "manual" ? t("schedules.manual") : formatWhen(new Date(run.window), locale) }}</span>
        <span v-if="duration(run)" class="history-meta">{{ duration(run) }}</span>
        <RouterLink v-if="run.sessionId" class="history-session" :to="{ name: 'chat-session', params: { sessionId: run.sessionId } }" @click="close">{{ t("schedules.openSession") }}</RouterLink>
        <p v-if="runDetail(run, t)" class="history-detail">{{ runDetail(run, t) }}</p>
      </li>
    </ol>
  </dialog>
</template>

<style scoped>
.history-empty { margin: var(--space-6) 0 var(--space-2); color: var(--muted-foreground); font-size: var(--text-sm); text-align: center; }
.history-list { display: grid; max-height: min(60vh, 520px); gap: 1px; margin: var(--space-4) calc(var(--space-2) * -1) 0; padding: 0; overflow-y: auto; list-style: none; }
.history-run { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2) var(--space-3); padding: var(--space-2); border-radius: var(--control-sm-radius); font-size: var(--text-sm); }
.history-run:hover { background: var(--hover); }
.history-when { font-variant-numeric: tabular-nums; }
.history-meta { color: var(--muted-foreground); font-variant-numeric: tabular-nums; }
.history-session { margin-left: auto; color: var(--primary); text-decoration: none; }
.history-session:hover { text-decoration: underline; }
.history-detail { width: 100%; margin: 0; color: var(--muted-foreground); font-size: var(--text-xs); overflow-wrap: anywhere; }
</style>
