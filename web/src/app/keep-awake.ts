import { computed, onBeforeUnmount, watch } from "vue"
import { useSessionState } from "../components/session-state"
import { canChooseWorkspace, keepAwake } from "../desktop"
import { useConnectionStore } from "../stores/connection"
import { usePreferencesStore } from "../stores/preferences"
import { useSessionsStore } from "../stores/sessions"

/** Runs this page did not start (a scheduled task's) show only in the directory, read this often meanwhile. */
const DIRECTORY_MS = 60_000

/**
 * While a Session runs, the desktop app keeps the computer from idle-sleeping,
 * as Claude Code's "Keep computer awake while Claude works" does; one waiting
 * on the operator does not count.
 */
export function useKeepAwake(): void {
  if (!canChooseWorkspace) return
  const preferences = usePreferencesStore()
  const sessions = useSessionsStore()
  const connection = useConnectionStore()
  const stateOf = useSessionState()
  const active = computed(() => preferences.keepAwake && sessions.summaries.some((session) => stateOf(session) === "running"))
  watch(active, (value) => keepAwake(value), { immediate: true })
  let timer: ReturnType<typeof setInterval> | undefined
  watch(() => preferences.keepAwake, (on) => {
    clearInterval(timer)
    timer = on ? setInterval(() => { if (connection.phase === "connected") void sessions.refresh() }, DIRECTORY_MS) : undefined
  }, { immediate: true })
  onBeforeUnmount(() => {
    clearInterval(timer)
    keepAwake(false)
  })
}
