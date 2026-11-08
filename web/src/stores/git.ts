import { computed, ref } from "vue"
import { defineStore } from "pinia"
import { readAck } from "../rpc/read"
import { readCreatedWorktree, readGitStatus, readPlaces, readPullRequest, type CreatedWorktree, type GitPlace, type GitPullRequestView, type GitStatus } from "../rpc/git"
import { useConnectionStore } from "./connection"

/** What Git says about the directories Sessions run in, and the worktrees new Sessions start in. */
export const useGitStore = defineStore("git", () => {
  const connection = useConnectionStore()
  /** Each directory looked up this page load, by path. */
  const places = ref<Record<string, GitPlace>>({})
  const available = computed(() => connection.supports("git.inspect"))
  const asking = new Set<string>()

  /** Looks up the directories not known yet; one request for all of them. */
  async function inspect(paths: readonly string[]): Promise<void> {
    const unknown = [...new Set(paths)].filter((path) => places.value[path] === undefined && !asking.has(path))
    if (!available.value || unknown.length === 0) return
    for (const path of unknown) asking.add(path)
    try {
      const found = await connection.client.call("git.inspect", { paths: unknown }, readPlaces)
      places.value = { ...places.value, ...Object.fromEntries(found.map((place) => [place.path, place])) }
    } finally {
      for (const path of unknown) asking.delete(path)
    }
  }

  /** The main worktree of the linked worktree rooted at `cwd`; undefined for any other directory. */
  function linkedMain(cwd: string): string | undefined {
    const place = places.value[cwd]
    return place?.root === cwd && place.main !== undefined && place.main !== cwd ? place.main : undefined
  }

  async function status(cwd: string): Promise<GitStatus> {
    const found = await connection.client.call("git.status", { cwd }, readGitStatus)
    const known = places.value[cwd]
    if (known !== undefined && known.branch !== found.branch) {
      const { branch: _, ...rest } = known
      places.value = { ...places.value, [cwd]: found.branch === undefined ? rest : { ...rest, branch: found.branch } }
    }
    return found
  }

  function pullRequest(cwd: string): Promise<GitPullRequestView> {
    return connection.client.call("git.pr", { cwd }, readPullRequest, 45_000)
  }

  async function createWorktree(cwd: string, branch: string): Promise<CreatedWorktree> {
    const created = await connection.client.call("git.worktree.create", { cwd, branch }, readCreatedWorktree, 60_000)
    places.value = { ...places.value, [created.path]: { path: created.path, root: created.path, main: places.value[cwd]?.main ?? cwd, branch: created.branch } }
    return created
  }

  async function removeWorktree(path: string): Promise<void> {
    await connection.client.call("git.worktree.remove", { path }, readAck, 60_000)
    const { [path]: _, ...rest } = places.value
    places.value = rest
  }

  return { places, available, inspect, linkedMain, status, pullRequest, createWorktree, removeWorktree }
})
