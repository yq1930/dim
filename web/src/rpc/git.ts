/** Readers of the `git.*` management replies (docs/design/web-workspace.md §4.5). */
import { arr, bool, fail, num, obj, optStr, str, type Json } from "./read-core"

/** Where a directory lies in Git; `root` is absent outside a repository. */
export type GitPlace = { path: string; root?: string; main?: string; branch?: string }

export type GitStatus = { root: string; main: string; branch?: string; upstream?: string; ahead: number; behind: number; changed: number; managed: boolean }

export type GitCheckState = "success" | "failure" | "pending" | "skipped" | "neutral"
export type GitCheck = { name: string; state: GitCheckState; url?: string }
export type GitPullRequest = { number: number; title: string; state: string; draft: boolean; url: string; base: string; head: string; review?: string; checks: GitCheck[] }
/** The branch's pull request; `null` when it has none, absent when `gh` cannot tell. */
export type GitPullRequestView = { available: true; pullRequest: GitPullRequest | null } | { available: false; reason: string; message: string }

export type CreatedWorktree = { path: string; branch: string }

const CHECK_STATES: readonly GitCheckState[] = ["success", "failure", "pending", "skipped", "neutral"]

function place(value: unknown, at: string): GitPlace {
  const source = obj(value, at)
  const found: GitPlace = { path: str(source, "path", at) }
  const root = optStr(source, "root", at)
  const main = optStr(source, "main", at)
  const branch = optStr(source, "branch", at)
  if (root !== undefined) found.root = root
  if (main !== undefined) found.main = main
  if (branch !== undefined) found.branch = branch
  return found
}

export function readPlaces(value: unknown): GitPlace[] {
  return arr(obj(value, "git.inspect").places, "git.inspect.places").map((item, index) => place(item, `git.inspect.places[${index}]`))
}

export function readGitStatus(value: unknown): GitStatus {
  const at = "git.status"
  const source = obj(value, at)
  const status: GitStatus = { root: str(source, "root", at), main: str(source, "main", at), ahead: num(source, "ahead", at), behind: num(source, "behind", at), changed: num(source, "changed", at), managed: bool(source, "managed", at) }
  const branch = optStr(source, "branch", at)
  const upstream = optStr(source, "upstream", at)
  if (branch !== undefined) status.branch = branch
  if (upstream !== undefined) status.upstream = upstream
  return status
}

function check(value: unknown, at: string): GitCheck {
  const source = obj(value, at)
  const state = str(source, "state", at)
  const known = CHECK_STATES.find((item) => item === state) ?? fail(`${at}.state`, "a check state")
  const found: GitCheck = { name: str(source, "name", at), state: known }
  const url = optStr(source, "url", at)
  if (url !== undefined) found.url = url
  return found
}

function pullRequest(source: Json, at: string): GitPullRequest {
  const found: GitPullRequest = {
    number: num(source, "number", at),
    title: str(source, "title", at),
    state: str(source, "state", at),
    draft: bool(source, "draft", at),
    url: str(source, "url", at),
    base: str(source, "base", at),
    head: str(source, "head", at),
    checks: arr(source.checks, `${at}.checks`).map((item, index) => check(item, `${at}.checks[${index}]`)),
  }
  const review = optStr(source, "review", at)
  if (review !== undefined) found.review = review
  return found
}

export function readPullRequest(value: unknown): GitPullRequestView {
  const at = "git.pr"
  const source = obj(value, at)
  if (!bool(source, "available", at)) return { available: false, reason: str(source, "reason", at), message: str(source, "message", at) }
  return { available: true, pullRequest: source.pullRequest === null ? null : pullRequest(obj(source.pullRequest, `${at}.pullRequest`), `${at}.pullRequest`) }
}

export function readCreatedWorktree(value: unknown): CreatedWorktree {
  const source = obj(value, "git.worktree.create")
  return { path: str(source, "path", "git.worktree.create"), branch: str(source, "branch", "git.worktree.create") }
}
