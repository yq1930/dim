import { computed, ref, watch } from "vue"
import { defineStore } from "pinia"
import { readAck, readCreate, readRenamedTitle, readSummaries } from "../rpc/read"
import { obj, str } from "../rpc/read-core"
import { translate } from "../i18n"
import type { ModelId, SessionSummary } from "../rpc/protocol"
import { useConnectionStore } from "./connection"
import { useGitStore } from "./git"

/**
 * Sessions that share a working directory, with those in its linked worktrees: the sidebar's workspace groups.
 * `cwd` is empty for the Sessions started without a folder, each in a scratch directory of its own.
 */
export type Workspace = { cwd: string; name: string; sessions: SessionSummary[]; updatedAt: number }

const RECENT_KEY = "dim-web.workspaces.v1"
function readRecentWorkspaces(): string[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]")
    return Array.isArray(value) ? [...new Set(value.filter((item): item is string => typeof item === "string"))].slice(0, 20) : []
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
  /** `<product home>/scratch` (web-workspace.md §4.11); undefined until known, and where the service has none. */
  const scratchRoot = ref<string>()

  /** Whether `cwd` is the scratch directory of a Session started without a folder. */
  function isScratch(cwd: string): boolean {
    const root = scratchRoot.value
    return root !== undefined && cwd.length > root.length + 1 && cwd.startsWith(root) && /[\\/]/.test(cwd[root.length]!)
  }

  const byId = computed(() => new Map(summaries.value.map((session) => [session.sessionId, session])))

  /** Workspaces by latest activity; sessions in each by latest activity. */
  const workspaces = computed<Workspace[]>(() => {
    const git = useGitStore()
    const workspaceOf = (cwd: string) => isScratch(cwd) ? "" : git.linkedMain(cwd) ?? cwd
    const groups = new Map<string, SessionSummary[]>()
    for (const session of summaries.value) {
      const cwd = workspaceOf(session.cwd)
      const group = groups.get(cwd)
      if (group === undefined) groups.set(cwd, [session])
      else group.push(session)
    }
    const saved = [...groups].map(([cwd, sessions]) => {
      sessions.sort((left, right) => right.updatedAt - left.updatedAt)
      return { cwd, name: workspaceName(cwd), sessions, updatedAt: sessions[0]!.updatedAt }
    }).sort((left, right) => right.updatedAt - left.updatedAt)
    const recentPaths = [...new Set(recentWorkspaces.value.map(workspaceOf))]
    const recent = recentPaths.map((cwd) => saved.find((workspace) => workspace.cwd === cwd) ?? { cwd, name: workspaceName(cwd), sessions: [], updatedAt: 0 })
    return [...recent, ...saved.filter((workspace) => !recentPaths.includes(workspace.cwd))]
  })

  /** Asks for the scratch root until the service names one. */
  async function findScratchRoot(): Promise<void> {
    const connection = useConnectionStore()
    if (scratchRoot.value !== undefined || !connection.supports("scratch.root")) return
    try {
      scratchRoot.value = await connection.client.call("scratch.root", {}, (value) => str(obj(value, "scratch.root"), "root", "scratch.root"))
      // A scratch directory remembered before the root was known stands for no folder.
      if (recentWorkspaces.value.some(isScratch)) recentWorkspaces.value = [...new Set(recentWorkspaces.value.map((path) => isScratch(path) ? "" : path))]
    } catch { /* without a product home there are no scratch directories */ }
  }

  /** Reloads the directory; concurrent callers share one request. */
  function refresh(): Promise<void> {
    const connection = useConnectionStore()
    // The scratch root comes with the first directory, so no scratch Session is listed as a workspace.
    refreshing ??= Promise.all([connection.client.call("session.summaries", connection.supports("session.archive") ? { archived: false } : {}, readSummaries), findScratchRoot()])
      .then(([list]) => {
        // A team member is listed under its leader's Members view, not among the user's Sessions.
        summaries.value = list.filter((session) => session.parentSessionId === undefined).map((session) => {
          const current = byId.value.get(session.sessionId)
          return current && (current.titleRevision ?? -1) > (session.titleRevision ?? -1)
            ? { ...session, title: current.title, titleRevision: current.titleRevision } : session
        })
        loaded.value = true
        error.value = ""
        void useGitStore().inspect([...list.map((session) => session.cwd), ...recentWorkspaces.value.filter((cwd) => cwd !== "")])
      })
      .catch((cause: unknown) => {
        error.value = cause instanceof Error ? cause.message : String(cause)
      })
      .finally(() => { refreshing = undefined })
    return refreshing
  }

  /**
   * Creates a session in `cwd` (an absolute directory), or with an empty `cwd` in a new scratch directory;
   * `model` overrides the directory's default.
   * `role` fixes the tools it mounts and adds to its system prompt, as a coordinator's.
   */
  async function create(cwd: string, model: ModelId | undefined, reasoningEffort?: string, role?: { tools: readonly string[]; appendSystemPrompt: string }): Promise<string> {
    const connection = useConnectionStore()
    const params: Record<string, unknown> = { cwd: cwd !== "" ? cwd : await connection.client.call("scratch.create", { name: datedName() }, (value) => str(obj(value, "scratch.create"), "path", "scratch.create")) }
    if (model !== undefined) params.model = { provider: model.providerId, modelId: model.modelId }
    if (reasoningEffort !== undefined) params.modelPreferences = { reasoningEffort }
    if (role !== undefined) {
      params.tools = role.tools
      params.appendSystemPrompt = role.appendSystemPrompt
    }
    const created = await connection.client.call("session.create", params, readCreate)
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

  /** Puts the workspace first among the recent ones; an empty `cwd` or a scratch directory stands for no folder. */
  function rememberWorkspace(cwd: string, previous?: string): void {
    const folder = isScratch(cwd) ? "" : cwd
    recentWorkspaces.value = [folder, ...recentWorkspaces.value.filter((path) => path !== folder && path !== previous)].slice(0, 20)
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

  return { summaries, loaded, error, byId, workspaces, scratchRoot, isScratch, refresh, create, setRunning, forget, setArchived, rememberWorkspace, forgetWorkspace, applyTitle, rename, remove }
})

/** What a working directory is shown as: its last segment, or "No folder" for none and for a scratch directory. */
export function workspaceName(cwd: string): string {
  if (cwd === "" || useSessionsStore().isScratch(cwd)) return translate("workspace.none")
  const trimmed = cwd.replace(/[\\/]+$/, "")
  return trimmed.split(/[\\/]/).at(-1) || cwd
}

/** Whether `path` is absolute on POSIX or Windows, as a Session's directory must be. */
export function isAbsolutePath(path: string): boolean {
  return path.startsWith("/") || /^[a-z]:[\\/]/i.test(path) || path.startsWith("\\\\")
}

/** Today's date and four hex digits: a name unlikely to be taken, for a new branch or scratch directory. */
export function datedName(): string {
  const now = new Date()
  const date = `${now.getFullYear()}${String(now.getMonth() + 1).padStart(2, "0")}${String(now.getDate()).padStart(2, "0")}`
  return `${date}-${Math.random().toString(16).slice(2, 6)}`
}
