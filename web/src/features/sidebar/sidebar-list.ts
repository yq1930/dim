import type { TranslationKey } from "../../i18n"
import type { SessionSummary } from "../../rpc/protocol"
import type { SidebarSortBy } from "../../stores/ui"
import type { SessionState } from "./session-state"

/** A headed run of sidebar rows. */
export type SessionGroup = { key: string; label: TranslationKey; sessions: SessionSummary[] }

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" })
const DAY_MS = 86_400_000

function name(session: SessionSummary): string {
  return session.title ?? session.preview ?? ""
}

/** By last activity, by name, or newest created first. */
export function sortSessions(sessions: readonly SessionSummary[], sortBy: SidebarSortBy): SessionSummary[] {
  return [...sessions].sort((left, right) => sortBy === "alpha"
    ? collator.compare(name(left), name(right))
    : sortBy === "created" ? right.createdAt - left.createdAt : right.updatedAt - left.updatedAt)
}

/** By the day of the last activity, or of creation when sorted by it: today, yesterday, the past week and older. */
export function groupByDate(sessions: readonly SessionSummary[], sortBy: SidebarSortBy, now: Date): SessionGroup[] {
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()
  const days = [
    { key: "today", label: "sidebar.date.today", from: today },
    { key: "yesterday", label: "sidebar.date.yesterday", from: today - DAY_MS },
    { key: "week", label: "sidebar.date.week", from: today - 6 * DAY_MS },
    { key: "older", label: "sidebar.date.older", from: -Infinity },
  ] as const
  const sorted = sortSessions(sessions, sortBy)
  const when = (session: SessionSummary) => sortBy === "created" ? session.createdAt : session.updatedAt
  return days.flatMap((day, index) => {
    const until = index === 0 ? Infinity : days[index - 1]!.from
    const within = sorted.filter((session) => when(session) >= day.from && when(session) < until)
    return within.length === 0 ? [] : [{ key: day.key, label: day.label, sessions: within }]
  })
}

/** Waiting on the operator first, then running, unread and the rest. */
export function groupByState(sessions: readonly SessionSummary[], sortBy: SidebarSortBy, stateOf: (session: SessionSummary) => SessionState): SessionGroup[] {
  const states = [
    { key: "waiting", label: "sidebar.state.waiting" },
    { key: "running", label: "sidebar.state.running" },
    { key: "unread", label: "sidebar.state.unread" },
    { key: "idle", label: "sidebar.state.idle" },
  ] as const
  const sorted = sortSessions(sessions, sortBy)
  return states.flatMap((state) => {
    const within = sorted.filter((session) => stateOf(session) === state.key)
    return within.length === 0 ? [] : [{ key: state.key, label: state.label, sessions: within }]
  })
}
