import { computed, onBeforeUnmount, watch } from "vue"
import { useI18n } from "vue-i18n"
import type { GitPullRequest } from "../../rpc/git"
import { useConnectionStore } from "../../stores/connection"
import { useGitStore } from "../../stores/git"
import { usePreferencesStore } from "../../stores/preferences"
import { useSessionsStore } from "../../stores/sessions"
import { useToastStore } from "../../stores/toast"

/**
 * Archives Sessions once their branch's pull request merges or closes, as
 * Claude Code's desktop app does. Only a pull request this browser saw open
 * (in a round here or in the Git tab) counts, so turning the setting on
 * leaves Sessions of long-merged branches alone. A Session running or shown
 * waits for a later round; uncommitted changes keep the Sessions for good.
 * After a merge, a worktree the workspace made is removed too; Git refuses
 * one with untracked files, and the branch stays.
 */
const SEEN_KEY = "dim-web.autoArchive.v1"
const FIRST_ROUND_MS = 30_000
const ROUND_MS = 5 * 60_000
/** A directory whose branch has no pull request is asked again after this. */
const QUIET_MS = 30 * 60_000
/** Directories asked per round, latest activity first. */
const MAX_DIRECTORIES = 30

/** Open pull requests seen, by directory. */
function readSeen(): Record<string, number> {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(SEEN_KEY) ?? "{}")
    if (value !== null && typeof value === "object" && !Array.isArray(value)) {
      return Object.fromEntries(Object.entries(value).filter((entry): entry is [string, number] => typeof entry[1] === "number"))
    }
  } catch { /* unreadable storage watches nothing */ }
  return {}
}

function updateSeen(cwd: string, number: number | undefined): void {
  const { [cwd]: previous, ...rest } = readSeen()
  if (previous === number) return
  try { localStorage.setItem(SEEN_KEY, JSON.stringify(number === undefined ? rest : { ...rest, [cwd]: number })) } catch { /* best effort */ }
}

/** Notes the pull request the Git tab read, so an open one is watched. */
export function notePullRequest(cwd: string, pullRequest: GitPullRequest | null): void {
  if (pullRequest?.state === "open") updateSeen(cwd, pullRequest.number)
}

/** Runs the rounds while the setting is on; `shown` lists the Sessions on screen. */
export function useAutoArchive(shown: () => readonly (string | undefined)[]): void {
  const { t } = useI18n()
  const preferences = usePreferencesStore()
  const connection = useConnectionStore()
  const sessions = useSessionsStore()
  const git = useGitStore()
  const toast = useToastStore()
  /** Directory and branch → when to ask again; a pull request closed before it was seen open is not asked again. */
  const quiet = new Map<string, number>()
  let timer: ReturnType<typeof setTimeout> | undefined
  let busy = false

  const enabled = computed(() => preferences.autoArchive && connection.phase === "connected" && ["git.pr", "git.status", "session.archive"].every((method) => connection.supports(method)))

  function schedule(delay: number): void {
    clearTimeout(timer)
    timer = enabled.value ? setTimeout(() => { void round() }, delay) : undefined
  }

  /** Archives the Sessions in `cwd` and returns how many, or "later" while one of them runs or is shown. */
  async function archive(cwd: string, merged: boolean): Promise<number | "later"> {
    const own = sessions.summaries.filter((session) => session.cwd === cwd)
    if (own.some((session) => session.running || shown().includes(session.sessionId))) return "later"
    const status = await git.status(cwd).catch(() => undefined)
    if (status === undefined) return "later"
    if (status.changed > 0) return 0
    let archived = 0
    for (const session of own) {
      try {
        await sessions.setArchived(session.sessionId, true)
        archived += 1
      } catch { /* left for the operator */ }
    }
    if (merged && status.managed && status.root === cwd && archived === own.length) {
      try {
        await git.removeWorktree(cwd)
        sessions.forgetWorkspace(cwd)
      } catch { /* Git keeps a worktree it will not remove */ }
    }
    return archived
  }

  async function round(): Promise<void> {
    if (busy) return
    busy = true
    try {
      const directories = [...new Set([...sessions.summaries].sort((left, right) => right.updatedAt - left.updatedAt).map((session) => session.cwd))]
        .filter((cwd) => git.places[cwd]?.branch !== undefined)
        .slice(0, MAX_DIRECTORIES)
      for (const cwd of directories) {
        if (!enabled.value) break
        const key = `${cwd}\n${git.places[cwd]?.branch}`
        const watched = readSeen()[cwd]
        if (watched === undefined && (quiet.get(key) ?? 0) > Date.now()) continue
        const view = await git.pullRequest(cwd).catch(() => undefined)
        if (view?.available !== true) continue
        const pullRequest = view.pullRequest
        if (pullRequest?.state === "open") {
          updateSeen(cwd, pullRequest.number)
          continue
        }
        if (pullRequest === null || pullRequest.number !== watched) {
          updateSeen(cwd, undefined)
          quiet.set(key, pullRequest === null ? Date.now() + QUIET_MS : Number.POSITIVE_INFINITY)
          continue
        }
        const merged = pullRequest.state === "merged"
        const archived = await archive(cwd, merged)
        if (archived === "later") continue
        updateSeen(cwd, undefined)
        if (archived > 0) toast.push({ tone: "success", title: t(merged ? "autoArchive.merged" : "autoArchive.closed", { number: pullRequest.number, count: archived }, archived) })
      }
    } finally {
      busy = false
      schedule(ROUND_MS)
    }
  }

  watch(enabled, () => schedule(FIRST_ROUND_MS), { immediate: true })
  onBeforeUnmount(() => clearTimeout(timer))
}
