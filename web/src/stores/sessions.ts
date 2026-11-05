import { computed, ref, watch } from "vue"
import { defineStore } from "pinia"
import { readAck, readCreate, readRenamedTitle, readSummaries } from "../rpc/read"
import type { ModelId, SessionSummary } from "../rpc/protocol"
import { useConnectionStore } from "./connection"

/** Sessions that share a working directory: the sidebar's workspace groups. */
export type Workspace = { cwd: string; name: string; sessions: SessionSummary[]; updatedAt: number }

const RECENT_KEY = "dim-web.workspaces.v1"
function readRecentWorkspaces(): string[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]")
    return Array.isArray(value) ? [...new Set(value.filter((item): item is string => typeof item === "string" && item.length > 0))].slice(0, 20) : []
  } catch { return [] }
}

/** The session directory (`session.summaries`) and session creation. */
export const useSessionsStore = defineStore("sessions", () => {
  const summaries = ref<SessionSummary[]>([])
  const recentWorkspaces = ref(readRecentWorkspaces())
  watch(recentWorkspaces, (value) => { try { localStorage.setItem(RECENT_KEY, JSON.stringify(value)) } catch {} })
  /** False until the first directory reply. */
  const loaded = ref(false)
  const error = ref("")
  let refreshing: Promise<void> | undefined

  const byId = computed(() => new Map(summaries.value.map((session) => [session.sessionId, session])))

  /** Workspaces by latest activity; sessions in each by latest activity. */
  const workspaces = computed<Workspace[]>(() => {
    const groups = new Map<string, SessionSummary[]>()
    for (const session of summaries.value) {
      const group = groups.get(session.cwd)
      if (group === undefined) groups.set(session.cwd, [session])
      else group.push(session)
    }
    const saved = [...groups].map(([cwd, sessions]) => {
      sessions.sort((left, right) => right.updatedAt - left.updatedAt)
      return { cwd, name: workspaceName(cwd), sessions, updatedAt: sessions[0]!.updatedAt }
    }).sort((left, right) => right.updatedAt - left.updatedAt)
    const recent = recentWorkspaces.value.map((cwd) => saved.find((workspace) => workspace.cwd === cwd) ?? { cwd, name: workspaceName(cwd), sessions: [], updatedAt: 0 })
    return [...recent, ...saved.filter((workspace) => !recentWorkspaces.value.includes(workspace.cwd))]
  })

  /** Reloads the directory; concurrent callers share one request. */
  function refresh(): Promise<void> {
    const connection = useConnectionStore()
    refreshing ??= connection.client.call("session.summaries", connection.supports("session.archive") ? { archived: false } : {}, readSummaries)
      .then((list) => {
        summaries.value = list.map((session) => {
          const current = byId.value.get(session.sessionId)
          return current && (current.titleRevision ?? -1) > (session.titleRevision ?? -1)
            ? { ...session, title: current.title, titleRevision: current.titleRevision } : session
        })
        loaded.value = true
        error.value = ""
      })
      .catch((cause: unknown) => {
        error.value = cause instanceof Error ? cause.message : String(cause)
      })
      .finally(() => { refreshing = undefined })
    return refreshing
  }

  /** Creates a session in `cwd` (an absolute directory); `model` overrides the directory's default. */
  async function create(cwd: string, model: ModelId | undefined, reasoningEffort?: string): Promise<string> {
    const params: Record<string, unknown> = { cwd }
    if (model !== undefined) params.model = { provider: model.providerId, modelId: model.modelId }
    if (reasoningEffort !== undefined) params.modelPreferences = { reasoningEffort }
    const created = await useConnectionStore().client.call("session.create", params, readCreate)
    void refresh().then(() => rememberWorkspace(byId.value.get(created.sessionId)?.cwd ?? cwd, cwd))
    return created.sessionId
  }

  /** Mirrors a live session's running state between directory refreshes. */
  function setRunning(sessionId: string, running: boolean): void {
    const session = byId.value.get(sessionId)
    if (session !== undefined && session.running !== running) session.running = running
  }

  function forget(sessionId: string): void {
    summaries.value = summaries.value.filter((session) => session.sessionId !== sessionId)
  }

  function rememberWorkspace(cwd: string, previous?: string): void {
    if (cwd) recentWorkspaces.value = [cwd, ...recentWorkspaces.value.filter((path) => path !== cwd && path !== previous)].slice(0, 20)
  }

  function forgetWorkspace(cwd: string): void {
    recentWorkspaces.value = recentWorkspaces.value.filter((path) => path !== cwd)
  }

  function applyTitle(sessionId: string, change: { title: string | undefined; titleRevision: number }): void {
    const session = byId.value.get(sessionId)
    if (session && change.titleRevision > (session.titleRevision ?? -1)) {
      session.title = change.title
      session.titleRevision = change.titleRevision
    }
  }

  async function rename(sessionId: string, title: string): Promise<void> {
    const changed = await useConnectionStore().client.call("session.rename", { sessionId, title }, readRenamedTitle)
    applyTitle(sessionId, changed)
  }

  async function remove(sessionId: string): Promise<void> {
    await refreshing
    await useConnectionStore().client.call("session.delete", { sessionId }, readAck)
    await refreshing
    forget(sessionId)
  }

  async function setArchived(sessionId: string, archived: boolean): Promise<void> {
    await refreshing
    await useConnectionStore().client.call("session.archive", { sessionId, archived }, readAck)
    if (archived) forget(sessionId)
    await refresh()
  }

  return { summaries, loaded, error, byId, workspaces, refresh, create, setRunning, forget, setArchived, rememberWorkspace, forgetWorkspace, applyTitle, rename, remove }
})

export function workspaceName(cwd: string): string {
  const trimmed = cwd.replace(/[\\/]+$/, "")
  return trimmed.split(/[\\/]/).at(-1) || cwd
}
