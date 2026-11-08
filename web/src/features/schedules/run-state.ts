import type { ScheduleRun, ScheduleRunState } from "../../rpc/schedule"

/** The badge tone of a run state; an absent tone is the neutral badge. */
export const RUN_TONE: Record<ScheduleRunState, "primary" | "success" | "warning" | "danger" | undefined> = {
  launching: "primary",
  submitted: "primary",
  running: "primary",
  completed: "success",
  skipped: "warning",
  missed: "warning",
  cancelled: undefined,
  failed: "danger",
  timed_out: "danger",
  lost: "danger",
}

/** Reasons the Scheduler records in a run's detail that read better in words (Scheduler A.1). */
const EXPLAINED = new Set(["overlap", "capacity", "disabled", "changed", "policy", "withdrawn", "withdrawn_at_deadline"])

/** What a run's detail adds to its state, if anything. */
export function runDetail(run: ScheduleRun, t: (key: string) => string): string | undefined {
  if (run.detail === "" || run.detail === run.state) return undefined
  return EXPLAINED.has(run.detail) ? t(`schedules.details.${run.detail}`) : run.detail
}
