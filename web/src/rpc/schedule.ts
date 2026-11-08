/**
 * Scheduled tasks: the `schedule.*` management methods of the Scheduler
 * plugin (docs/design/plugins/Scheduler.md, A.1–A.4), read at this boundary.
 */
import type { ModelId, PermissionPreset } from "./protocol"
import { fail, obj, arr, str, optStr, num, bool, modelId, type Json } from "./read-core"

/** When a job runs, as the server stores it: a cron line, a fixed interval, or one instant. */
export type ScheduleSpec = { cron: string; tz?: string } | { every_minutes: number } | { at: string }

export type ScheduleRunState = "skipped" | "missed" | "launching" | "submitted" | "running" | "completed" | "failed" | "cancelled" | "timed_out" | "lost"

export type ScheduleRun = {
  runId: string
  /** `manual` for a run started by hand, else the window's instant. */
  window: string
  state: ScheduleRunState
  sessionId?: string
  createdAt: string
  endedAt?: string
  detail: string
}

/** `create` starts a Session for every run; `reuse` keeps sending to the one its first run made. */
export type ScheduleMode = "create" | "reuse"

export type ScheduleJob = {
  id: string
  name: string
  schedule: ScheduleSpec
  prompt: string
  cwd: string
  model?: ModelId
  mode: ScheduleMode
  policy?: PermissionPreset
  enabled: boolean
  maxRunMinutes: number
  revision: number
  /** Listed jobs only: the next window, or why the schedule has none. */
  nextWindow?: string
  scheduleError?: string
  lastRun?: ScheduleRun
}

const STATES: readonly ScheduleRunState[] = ["skipped", "missed", "launching", "submitted", "running", "completed", "failed", "cancelled", "timed_out", "lost"]
const PRESETS: readonly PermissionPreset[] = ["read-only", "ask", "workspace-write", "full-access"]

function readSpec(value: unknown, at: string): ScheduleSpec {
  const source = obj(value, at)
  if (typeof source.cron === "string") return { cron: source.cron, ...(source.tz === undefined ? {} : { tz: str(source, "tz", at) }) }
  if (typeof source.every_minutes === "number") return { every_minutes: num(source, "every_minutes", at) }
  if (typeof source.at === "string") return { at: source.at }
  return fail(at, "a cron, every_minutes or at schedule")
}

function readRun(value: unknown, at: string): ScheduleRun {
  const source = obj(value, at)
  const state = STATES.find((name) => name === source.state) ?? fail(`${at}.state`, "a run state")
  const sessionId = optStr(source, "session_id", at)
  const endedAt = optStr(source, "ended_at", at)
  return {
    runId: str(source, "run_id", at),
    window: str(source, "window", at),
    state,
    ...(sessionId === undefined ? {} : { sessionId }),
    createdAt: str(source, "created_at", at),
    ...(endedAt === undefined ? {} : { endedAt }),
    detail: str(source, "detail", at),
  }
}

function readJob(value: unknown, at: string): ScheduleJob {
  const source: Json = obj(value, at)
  const mode = source.mode === "create" || source.mode === "reuse" ? source.mode : fail(`${at}.mode`, "create or reuse")
  const policy = source.policy === undefined ? undefined : PRESETS.find((preset) => preset === source.policy) ?? fail(`${at}.policy`, "a permission preset")
  const nextWindow = optStr(source, "next_window", at)
  const scheduleError = optStr(source, "schedule_error", at)
  return {
    id: str(source, "id", at),
    name: str(source, "name", at),
    schedule: readSpec(source.schedule, `${at}.schedule`),
    prompt: str(source, "prompt", at),
    cwd: str(source, "cwd", at),
    ...(source.model === undefined ? {} : { model: modelId(source.model, `${at}.model`) }),
    mode,
    ...(policy === undefined ? {} : { policy }),
    enabled: bool(source, "enabled", at),
    maxRunMinutes: num(source, "max_run_minutes", at),
    revision: num(source, "revision", at),
    ...(nextWindow === undefined ? {} : { nextWindow }),
    ...(scheduleError === undefined ? {} : { scheduleError }),
    ...(source.last_run === undefined || source.last_run === null ? {} : { lastRun: readRun(source.last_run, `${at}.last_run`) }),
  }
}

/** `schedule.list`: every job with its next window and latest run. */
export function readScheduleJobs(value: unknown): ScheduleJob[] {
  const at = "schedule.list"
  return arr(obj(value, at).jobs, `${at}.jobs`).map((item, index) => readJob(item, `${at}.jobs[${index}]`))
}

/** `schedule.save` and `schedule.set_enabled`: the job as stored. */
export function readSavedJob(value: unknown): ScheduleJob {
  return readJob(obj(value, "schedule.job").job, "schedule.job.job")
}

/** `schedule.history`: the job's runs, newest first. */
export function readScheduleRuns(value: unknown): ScheduleRun[] {
  const at = "schedule.history"
  return arr(obj(value, at).runs, `${at}.runs`).map((item, index) => readRun(item, `${at}.runs[${index}]`))
}

/** `schedule.run_now`: the run it started, or recorded as skipped. */
export function readScheduleRun(value: unknown): ScheduleRun {
  return readRun(value, "schedule.run_now")
}
