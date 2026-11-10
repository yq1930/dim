<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, useId } from "vue"
import { useI18n } from "vue-i18n"
import { FolderOpen } from "lucide-vue-next"
import { canChooseWorkspace, chooseWorkspace } from "../../desktop"
import type { PermissionPreset } from "../../rpc/protocol"
import { readSavedJob, type ScheduleJob, type ScheduleMode } from "../../rpc/schedule"
import { useConnectionStore } from "../../stores/connection"
import { useModelsStore } from "../../stores/models"
import { useSessionsStore } from "../../stores/sessions"
import { formFrom, specFrom, weekdayName, type ScheduleForm } from "./schedule-form"

/** Creates or edits a scheduled task; saving replaces its authority with the host's (Scheduler §3.3). */
const emit = defineEmits<{ saved: [job: ScheduleJob] }>()
const { t, locale } = useI18n()
const connection = useConnectionStore()
const models = useModelsStore()
const sessions = useSessionsStore()
const id = useId()
const dialog = ref<HTMLDialogElement>()
const nameField = ref<HTMLInputElement>()
const editing = ref<ScheduleJob>()
const name = ref("")
const prompt = ref("")
const cwd = ref("")
const form = ref<ScheduleForm>(formFrom(undefined))
const policy = ref<PermissionPreset>("read-only")
const model = ref("")
const mode = ref<ScheduleMode>("create")
const maxRunMinutes = ref(60)
const enabled = ref(true)
const saving = ref(false)
const error = ref("")
let generation = 0

/** Runs answer no approvals, so `ask` is not offered; a job without a policy runs read-only (Scheduler §3.3). */
const POLICIES = { "read-only": "schedules.policies.readOnly", "workspace-write": "schedules.policies.workspaceWrite", "full-access": "schedules.policies.fullAccess" } as const
const spec = computed(() => specFrom(form.value))
const workspaces = computed(() => sessions.workspaces.flatMap((workspace) => workspace.cwd || []))
const weekdays = computed(() => [1, 2, 3, 4, 5, 6, 0].map((day) => ({ day, label: weekdayName(day, locale.value) })))
const modelChoices = computed(() => (models.providers ?? []).map((provider) => ({ name: provider.name, models: provider.models.map((entry) => ({ key: `${provider.providerId}\n${entry.id}`, name: entry.name })) })))
const valid = computed(() => name.value.trim() !== "" && prompt.value.trim() !== "" && cwd.value.trim() !== "" && spec.value !== undefined && maxRunMinutes.value >= 1)

async function open(job?: ScheduleJob, folder = ""): Promise<void> {
  ++generation
  editing.value = job
  name.value = job?.name ?? ""
  prompt.value = job?.prompt ?? ""
  cwd.value = job?.cwd ?? folder
  form.value = formFrom(job?.schedule)
  policy.value = job?.policy ?? "read-only"
  model.value = job?.model === undefined ? "" : `${job.model.providerId}\n${job.model.modelId}`
  mode.value = job?.mode ?? "create"
  maxRunMinutes.value = job?.maxRunMinutes ?? 60
  enabled.value = job?.enabled ?? true
  saving.value = false
  error.value = ""
  dialog.value?.showModal()
  await nextTick()
  nameField.value?.focus()
  if (models.available) void models.load().catch(() => undefined)
}

function close(): void {
  ++generation
  dialog.value?.close()
}

async function pickFolder(): Promise<void> {
  const chosen = await chooseWorkspace(cwd.value).catch(() => null)
  if (chosen) cwd.value = chosen
}

async function save(): Promise<void> {
  const schedule = spec.value
  if (!valid.value || saving.value || schedule === undefined) return
  const request = generation
  const [providerId, modelId] = model.value.split("\n")
  saving.value = true
  error.value = ""
  try {
    const job = await connection.client.call("schedule.save", {
      job: {
        ...(editing.value === undefined ? {} : { id: editing.value.id }),
        name: name.value.trim(),
        prompt: prompt.value,
        cwd: cwd.value.trim(),
        schedule,
        mode: mode.value,
        policy: policy.value,
        ...(providerId && modelId ? { model: { providerId, modelId } } : {}),
        max_run_minutes: Math.round(maxRunMinutes.value),
        enabled: enabled.value,
      },
      ...(editing.value === undefined ? {} : { revision: editing.value.revision }),
    }, readSavedJob)
    if (request !== generation) return
    emit("saved", job)
    close()
  } catch (cause) {
    if (request === generation) error.value = cause instanceof Error ? cause.message : String(cause)
  } finally {
    if (request === generation) saving.value = false
  }
}

onBeforeUnmount(close)
defineExpose({ open })
</script>

<template>
  <dialog ref="dialog" class="ui-dialog schedule-editor" data-size="lg" :aria-labelledby="id" @cancel.prevent="close">
    <form @submit.prevent="save">
      <header class="ui-dialog-header"><h2 :id="id" class="ui-dialog-title">{{ editing ? t("schedules.editTitle") : t("schedules.new") }}</h2></header>
      <div class="schedule-fields">
        <label><span class="ui-label">{{ t("schedules.name") }}</span><input ref="nameField" v-model="name" class="ui-field" maxlength="256" required /></label>
        <label><span class="ui-label">{{ t("schedules.prompt") }}</span><textarea v-model="prompt" class="ui-field" rows="4" required /></label>
        <label>
          <span class="ui-label">{{ t("schedules.workspace") }}</span>
          <span class="schedule-row">
            <input v-model="cwd" class="ui-field" list="schedule-workspaces" spellcheck="false" required />
            <button v-if="canChooseWorkspace" type="button" class="ui-icon-button" data-variant="secondary" data-size="md" :aria-label="t('schedules.chooseFolder')" v-tooltip="t('schedules.chooseFolder')" @click="pickFolder"><FolderOpen aria-hidden="true" /></button>
          </span>
          <datalist id="schedule-workspaces"><option v-for="path in workspaces" :key="path" :value="path" /></datalist>
        </label>
        <div>
          <span class="ui-label">{{ t("schedules.frequency") }}</span>
          <span class="schedule-row">
            <select v-model="form.frequency" class="ui-field" :aria-label="t('schedules.frequency')">
              <option v-for="frequency in ['daily', 'weekdays', 'weekly', 'interval', 'once', 'cron'] as const" :key="frequency" :value="frequency">{{ t(`schedules.frequencies.${frequency}`) }}</option>
            </select>
            <select v-if="form.frequency === 'weekly'" v-model="form.weekday" class="ui-field" :aria-label="t('schedules.weekday')">
              <option v-for="weekday in weekdays" :key="weekday.day" :value="weekday.day">{{ weekday.label }}</option>
            </select>
            <input v-if="['daily', 'weekdays', 'weekly'].includes(form.frequency)" v-model="form.time" class="ui-field" type="time" :aria-label="t('schedules.time')" required />
            <template v-else-if="form.frequency === 'interval'">
              <input v-model.number="form.interval" class="ui-field" type="number" min="1" :aria-label="t('schedules.interval')" required />
              <select v-model="form.intervalUnit" class="ui-field" :aria-label="t('schedules.interval')">
                <option value="minutes">{{ t("schedules.minutes") }}</option>
                <option value="hours">{{ t("schedules.hours") }}</option>
              </select>
            </template>
            <input v-else-if="form.frequency === 'once'" v-model="form.at" class="ui-field" type="datetime-local" :aria-label="t('schedules.at')" required />
            <input v-else-if="form.frequency === 'cron'" v-model="form.cron" class="ui-field" spellcheck="false" placeholder="0 9 * * 1-5" :aria-label="t('schedules.cron')" required />
          </span>
        </div>
        <div class="schedule-grid">
          <label>
            <span class="ui-label">{{ t("schedules.permission") }}</span>
            <select v-model="policy" class="ui-field">
              <option v-for="(label, preset) in POLICIES" :key="preset" :value="preset">{{ t(label) }}</option>
            </select>
          </label>
          <label>
            <span class="ui-label">{{ t("schedules.model") }}</span>
            <select v-model="model" class="ui-field">
              <option v-if="editing?.model === undefined" value="">{{ t("schedules.defaultModel") }}</option>
              <option v-if="model !== '' && !modelChoices.some((group) => group.models.some((entry) => entry.key === model))" :value="model">{{ model.split("\n")[1] }}</option>
              <optgroup v-for="group in modelChoices" :key="group.name" :label="group.name">
                <option v-for="entry in group.models" :key="entry.key" :value="entry.key">{{ entry.name }}</option>
              </optgroup>
            </select>
          </label>
          <label>
            <span class="ui-label">{{ t("schedules.session") }}</span>
            <select v-model="mode" class="ui-field">
              <option value="create">{{ t("schedules.modes.create") }}</option>
              <option value="reuse">{{ t("schedules.modes.reuse") }}</option>
            </select>
          </label>
          <label>
            <span class="ui-label">{{ t("schedules.maxRun") }}</span>
            <input v-model.number="maxRunMinutes" class="ui-field" type="number" min="1" required />
          </label>
        </div>
        <label class="schedule-enabled"><input v-model="enabled" class="ui-check" type="checkbox" />{{ t("schedules.enabled") }}</label>
        <p v-if="error" class="ui-banner" data-tone="danger" role="alert">{{ error }}</p>
      </div>
      <footer class="ui-dialog-footer">
        <button type="button" class="ui-button" data-variant="secondary" @click="close">{{ t("common.cancel") }}</button>
        <button class="ui-button" data-variant="primary" :disabled="!valid || saving || connection.phase !== 'connected'">{{ t("common.save") }}</button>
      </footer>
    </form>
  </dialog>
</template>

<style scoped>
.schedule-fields { display: grid; gap: var(--space-3); margin-top: var(--space-4); }
.schedule-fields label { display: block; }
.schedule-row { display: flex; gap: var(--space-2); }
.schedule-row > .ui-field { min-width: 0; flex: 1; }
.schedule-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: var(--space-3); }
.schedule-enabled { display: flex !important; align-items: center; gap: var(--space-2); font-size: var(--text-sm); }
@media (max-width: 560px) { .schedule-grid { grid-template-columns: minmax(0, 1fr); } }
</style>
