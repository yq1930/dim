import type { ScheduleSpec } from "../../rpc/schedule"

/** The frequencies the editor offers; anything else a cron line can say is `cron`. */
export type Frequency = "daily" | "weekdays" | "weekly" | "interval" | "once" | "cron"

export type ScheduleForm = {
  frequency: Frequency
  /** `HH:MM` of a daily, weekday or weekly run. */
  time: string
  /** 0 is Sunday, as cron counts. */
  weekday: number
  interval: number
  intervalUnit: "minutes" | "hours"
  /** A `datetime-local` value for a single run. */
  at: string
  cron: string
}

const SIMPLE_CRON = /^(\d{1,2}) (\d{1,2}) \* \* (\*|1-5|[0-6])$/

function pad(value: number): string {
  return String(value).padStart(2, "0")
}

function localInput(date: Date): string {
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`
}

/** The editor's fields for a stored schedule, or a daily 09:00 run for a new job. */
export function formFrom(spec: ScheduleSpec | undefined, now = new Date()): ScheduleForm {
  const tomorrow = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1, 9, 0)
  const form: ScheduleForm = { frequency: "daily", time: "09:00", weekday: 1, interval: 30, intervalUnit: "minutes", at: localInput(tomorrow), cron: "0 9 * * *" }
  if (spec === undefined) return form
  if ("every_minutes" in spec) {
    const hours = spec.every_minutes % 60 === 0
    return { ...form, frequency: "interval", interval: hours ? spec.every_minutes / 60 : spec.every_minutes, intervalUnit: hours ? "hours" : "minutes" }
  }
  if ("at" in spec) return { ...form, frequency: "once", at: localInput(new Date(spec.at)) }
  const match = spec.tz === undefined ? SIMPLE_CRON.exec(spec.cron) : null
  if (match === null || Number(match[1]) > 59 || Number(match[2]) > 23) return { ...form, frequency: "cron", cron: spec.cron }
  const time = `${pad(Number(match[2]))}:${pad(Number(match[1]))}`
  if (match[3] === "*") return { ...form, frequency: "daily", time, cron: spec.cron }
  if (match[3] === "1-5") return { ...form, frequency: "weekdays", time, cron: spec.cron }
  return { ...form, frequency: "weekly", time, weekday: Number(match[3]), cron: spec.cron }
}

/** The schedule the fields describe, or undefined while they are incomplete. */
export function specFrom(form: ScheduleForm): ScheduleSpec | undefined {
  const clock = /^(\d{2}):(\d{2})$/.exec(form.time)
  const daily = clock === null ? undefined : `${Number(clock[2])} ${Number(clock[1])} * *`
  switch (form.frequency) {
    case "daily": return daily === undefined ? undefined : { cron: `${daily} *` }
    case "weekdays": return daily === undefined ? undefined : { cron: `${daily} 1-5` }
    case "weekly": return daily === undefined ? undefined : { cron: `${daily} ${form.weekday}` }
    case "interval": {
      const minutes = Math.round(form.interval) * (form.intervalUnit === "hours" ? 60 : 1)
      return Number.isFinite(minutes) && minutes >= 1 ? { every_minutes: minutes } : undefined
    }
    case "once": {
      const at = new Date(form.at)
      return Number.isNaN(at.getTime()) ? undefined : { at: at.toISOString() }
    }
    case "cron": return form.cron.trim().split(/\s+/).length === 5 ? { cron: form.cron.trim().split(/\s+/).join(" ") } : undefined
  }
}

export function weekdayName(weekday: number, locale: string): string {
  return new Intl.DateTimeFormat(locale, { weekday: "long" }).format(new Date(2024, 0, 7 + weekday))
}

/** A time near now as a person says it: today, tomorrow or a date, then the clock. */
export function formatWhen(at: Date, locale: string, now = new Date()): string {
  const day = (date: Date) => new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime()
  const days = Math.round((day(at) - day(now)) / 86_400_000)
  const clock = new Intl.DateTimeFormat(locale, { hour: "2-digit", minute: "2-digit", hour12: false }).format(at)
  const date = Math.abs(days) <= 1
    ? new Intl.RelativeTimeFormat(locale, { numeric: "auto" }).format(days, "day")
    : new Intl.DateTimeFormat(locale, { ...(at.getFullYear() === now.getFullYear() ? {} : { year: "numeric" }), month: "short", day: "numeric" }).format(at)
  return `${date} ${clock}`
}
