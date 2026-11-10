<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue"
import { useI18n } from "vue-i18n"
import { CalendarClock, History, Menu, Pencil, Play, Plus, Trash2 } from "lucide-vue-next"
import ConfirmDialog from "../../components/ConfirmDialog.vue"
import RowActionMenu from "../../components/RowActionMenu.vue"
import { readAck } from "../../rpc/read"
import { readSavedJob, readScheduleJobs, readScheduleRun, type ScheduleJob, type ScheduleSpec } from "../../rpc/schedule"
import { useConnectionStore } from "../../stores/connection"
import { useSessionsStore, workspaceName } from "../../stores/sessions"
import { useToastStore } from "../../stores/toast"
import { useUiStore } from "../../stores/ui"
import ScheduleEditor from "./ScheduleEditor.vue"
import ScheduleHistory from "./ScheduleHistory.vue"
import { formFrom, formatWhen, weekdayName } from "./schedule-form"
import { RUN_TONE, runDetail } from "./run-state"

/** Scheduled tasks: prompts the Scheduler sends to a Session on their schedule (Scheduler A.3). */
const { t, locale } = useI18n()
const connection = useConnectionStore()
const toast = useToastStore()
const ui = useUiStore()
const sessions = useSessionsStore()
const editor = ref<InstanceType<typeof ScheduleEditor>>()
const history = ref<InstanceType<typeof ScheduleHistory>>()
const confirm = ref<InstanceType<typeof ConfirmDialog>>()
const jobs = ref<ScheduleJob[]>([])
const loaded = ref(false)
const error = ref("")
const busy = ref<string>()
const REFRESH_MS = 30_000
/** While a run is under way its state changes within seconds. */
const ACTIVE_REFRESH_MS = 3_000
let generation = 0
let timer: ReturnType<typeof setInterval> | undefined
let soon: ReturnType<typeof setTimeout> | undefined
/** The latest runs last read; a change means a run made or used a Session the sidebar should show. */
let seenRuns: string | undefined

function message(cause: unknown): string {
  return cause instanceof Error && cause.message ? cause.message : String(cause)
}

async function load(): Promise<void> {
  if (connection.phase !== "connected") return
  const request = ++generation
  try {
    const found = await connection.client.call("schedule.list", {}, readScheduleJobs)
    if (request !== generation) return
    jobs.value = found
    error.value = ""
    const runs = found.map((job) => `${job.lastRun?.runId}:${job.lastRun?.state}`).join()
    if (seenRuns !== undefined && runs !== seenRuns) void sessions.refresh()
    seenRuns = runs
    clearTimeout(soon)
    if (found.some((job) => job.lastRun !== undefined && ["launching", "submitted", "running"].includes(job.lastRun.state))) soon = setTimeout(() => { void load() }, ACTIVE_REFRESH_MS)
  } catch (cause) {
    if (request === generation) error.value = message(cause)
  } finally {
    if (request === generation) loaded.value = true
  }
}

function describe(spec: ScheduleSpec): string {
  const form = formFrom(spec)
  switch (form.frequency) {
    case "daily": return t("schedules.describe.daily", { time: form.time })
    case "weekdays": return t("schedules.describe.weekdays", { time: form.time })
    case "weekly": return t("schedules.describe.weekly", { day: weekdayName(form.weekday, locale.value), time: form.time })
    case "interval": return t(form.intervalUnit === "hours" ? "schedules.describe.hours" : "schedules.describe.minutes", { count: form.interval })
    case "once": return t("schedules.describe.once", { time: "at" in spec ? formatWhen(new Date(spec.at), locale.value) : form.at })
    case "cron": return t("schedules.describe.cron", { cron: "cron" in spec ? `${spec.cron}${spec.tz ? ` (${spec.tz})` : ""}` : form.cron })
  }
}

async function act(job: ScheduleJob, work: () => Promise<void>): Promise<void> {
  if (busy.value !== undefined) return
  busy.value = job.id
  try {
    await work()
  } catch (cause) {
    toast.push({ tone: "danger", title: t("schedules.actionFailed"), description: message(cause) })
  } finally {
    busy.value = undefined
    await load()
  }
}

function toggle(job: ScheduleJob): Promise<void> {
  return act(job, async () => { await connection.client.call("schedule.set_enabled", { job_id: job.id, enabled: !job.enabled }, readSavedJob) })
}

function runNow(job: ScheduleJob): Promise<void> {
  return act(job, async () => {
    const run = await connection.client.call("schedule.run_now", { job_id: job.id }, readScheduleRun)
    if (run.state === "skipped" || run.state === "missed") toast.push({ tone: "info", title: t("schedules.skipped"), description: runDetail(run, t) })
    else toast.push({ tone: "success", title: t("schedules.started"), description: job.name })
  })
}

async function remove(job: ScheduleJob): Promise<void> {
  if (!await confirm.value?.open(t("schedules.deleteConfirm", { name: job.name }), { confirmLabel: t("schedules.delete") })) return
  await act(job, async () => { await connection.client.call("schedule.delete", { job_id: job.id }, readAck) })
}

/** A new task starts in the workspace of the Session last shown, unless that Session has no folder. */
function create(): void {
  const last = ui.lastSessionId === undefined ? undefined : sessions.byId.get(ui.lastSessionId)?.cwd
  void editor.value?.open(undefined, last === undefined || sessions.isScratch(last) ? "" : last)
}

watch(() => connection.phase, (phase) => { if (phase === "connected") void load() }, { immediate: true })
onMounted(() => { timer = setInterval(() => { void load() }, REFRESH_MS) })
onBeforeUnmount(() => {
  clearInterval(timer)
  clearTimeout(soon)
  ++generation
})
</script>

<template>
  <section class="schedules-page">
    <header class="session-header" data-tauri-drag-region="deep">
      <button v-if="ui.shellMode === 'compact'" class="ui-icon-button session-nav" data-variant="ghost" data-size="md" type="button" :aria-label="t('sidebar.openMenu')" @click="ui.drawerOpen = true"><Menu aria-hidden="true" /></button>
      <h1>{{ t("schedules.title") }}</h1>
      <button v-if="jobs.length > 0" type="button" class="ui-button schedules-new" data-variant="secondary" data-size="sm" :disabled="connection.phase !== 'connected'" @click="create"><Plus aria-hidden="true" />{{ t("schedules.new") }}</button>
    </header>
    <div class="schedules-body app-scroll-viewport">
      <p v-if="error" class="ui-banner" data-tone="danger" role="alert">{{ error }}</p>
      <div v-if="loaded && jobs.length === 0 && !error" class="schedules-empty">
        <CalendarClock :size="28" aria-hidden="true" />
        <p>{{ t("schedules.empty") }}</p>
        <button type="button" class="ui-button" data-variant="primary" data-size="sm" :disabled="connection.phase !== 'connected'" @click="create"><Plus aria-hidden="true" />{{ t("schedules.new") }}</button>
      </div>
      <ul v-else class="schedule-list">
        <li v-for="job in jobs" :key="job.id" class="schedule-item" :data-enabled="job.enabled">
          <button type="button" class="schedule-main" @click="history?.open(job)">
            <strong class="schedule-name">{{ job.name }}</strong>
            <span class="schedule-meta">
              <span>{{ describe(job.schedule) }}</span>
              <span :title="job.cwd">{{ workspaceName(job.cwd) }}</span>
              <span v-if="job.scheduleError" class="schedule-problem">{{ job.scheduleError }}</span>
              <span v-else-if="!job.enabled">{{ t("schedules.paused") }}</span>
              <span v-else-if="job.nextWindow">{{ t("schedules.next", { time: formatWhen(new Date(job.nextWindow), locale) }) }}</span>
            </span>
            <span v-if="job.lastRun" class="schedule-last">
              <span class="ui-badge" data-size="sm" :data-tone="RUN_TONE[job.lastRun.state]">{{ t(`schedules.states.${job.lastRun.state}`) }}</span>
              <span>{{ formatWhen(new Date(job.lastRun.createdAt), locale) }}</span>
            </span>
          </button>
          <div class="schedule-actions">
            <button type="button" role="switch" class="ui-switch" :aria-checked="job.enabled" :aria-label="t('schedules.enabled')" v-tooltip="job.enabled ? t('schedules.pause') : t('schedules.resume')" :disabled="busy !== undefined" @click="toggle(job)" />
            <button type="button" class="ui-button" data-variant="ghost" data-size="sm" :disabled="busy !== undefined || !job.enabled" @click="runNow(job)"><Play aria-hidden="true" />{{ t("schedules.runNow") }}</button>
            <RowActionMenu :label="t('schedules.more')">
              <button type="button" class="ui-menu-item" @click="editor?.open(job)"><Pencil aria-hidden="true" />{{ t("schedules.edit") }}</button>
              <button type="button" class="ui-menu-item" @click="history?.open(job)"><History aria-hidden="true" />{{ t("schedules.history") }}</button>
              <button type="button" class="ui-menu-item" data-tone="danger" @click="remove(job)"><Trash2 aria-hidden="true" />{{ t("schedules.delete") }}</button>
            </RowActionMenu>
          </div>
        </li>
      </ul>
    </div>
    <ScheduleEditor ref="editor" @saved="load" />
    <ScheduleHistory ref="history" />
    <ConfirmDialog ref="confirm" />
  </section>
</template>

<style scoped>
.schedules-page { display: grid; min-width: 0; min-height: 0; grid-template-rows: auto minmax(0, 1fr); overflow: hidden; }
.schedules-new { grid-area: panel; }
.schedules-page > .session-header > h1 { grid-area: 1 / 2 / 3 / 4; }
.schedules-body { min-height: 0; overflow-y: auto; padding: var(--space-6) clamp(var(--space-4), 4vw, var(--space-8)); }
.schedules-body > * { max-width: 880px; margin-inline: auto; }
.schedules-empty { display: grid; justify-items: center; gap: var(--space-3); margin-top: 12vh; color: var(--muted-foreground); font-size: var(--text-sm); }
.schedules-empty p { margin: 0; }
.schedule-list { display: grid; gap: var(--space-2); padding: 0; list-style: none; }
.schedule-item { display: flex; align-items: center; gap: var(--space-3); padding: var(--space-3) var(--space-3) var(--space-3) var(--space-4); border: 1px solid var(--border); border-radius: var(--radius-lg); background: var(--card); }
.schedule-item[data-enabled="false"] .schedule-main { opacity: .6; }
.schedule-main { display: grid; min-width: 0; flex: 1; gap: 4px; padding: 0; border: 0; background: none; color: inherit; font: inherit; text-align: left; cursor: pointer; }
.schedule-name { overflow: hidden; font-size: var(--text-sm); font-weight: var(--weight-medium); text-overflow: ellipsis; white-space: nowrap; }
.schedule-meta, .schedule-last { display: flex; min-width: 0; flex-wrap: wrap; align-items: center; gap: 2px var(--space-3); color: var(--muted-foreground); font-size: var(--text-xs); }
.schedule-problem { color: var(--danger); }
.schedule-actions { display: flex; flex: none; align-items: center; gap: var(--space-2); }
</style>
