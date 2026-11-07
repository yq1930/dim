import { shallowReactive } from "vue"
import { defineStore } from "pinia"
import { Projection, type TimelineRow } from "../projection"
import { Lines, type LineView } from "../lines"
import { ConnectionLost, RpcError } from "../rpc/client"
import type { Goal } from "../goal"
import { canChooseWorkspace as inDesktopShell, notifyDesktop } from "../desktop"
import { translate, type TranslationKey } from "../i18n"
import { MAIN_BRANCH, type ApprovalRequest, type ModelId, type Notification, type PendingPlan, type PendingQuestion, type PermissionPolicy, type QueuedInput, type SessionEvent, type TodoItem } from "../rpc/protocol"
import { ProtocolError, readAck, readHistory } from "../rpc/read"
import { removeApproval, sessionCalls } from "./session-calls"
import { useConnectionStore } from "./connection"
import { useSessionsStore } from "./sessions"
import { useUiStore } from "./ui"

/** What a permission change did: written, already in place, or refused because `main` changed first. */
export type PermissionOutcome = "updated" | "unchanged" | "conflict"

/** One line's transcript and history state. */
export type BranchView = {
  rows: readonly TimelineRow[]
  running: boolean
  stopping: boolean
  status: "loading" | "ready" | "failed"
  error: string
  lastTurnEnd: number | undefined
}

/** What the chat page shows of one attached session. */
export type SessionView = {
  sessionId: string
  /** `loading` until the first main-history load finishes; `closed` once the session is gone. */
  status: "loading" | "ready" | "closed" | "failed"
  error: string
  /** The main line's transcript. */
  rows: readonly TimelineRow[]
  /** Whether main runs, and whether it is stopping. */
  running: boolean
  stopping: boolean
  /** Whether any line of this session runs: the header and sidebar badge. */
  sessionRunning: boolean
  /** Pending approvals of every branch, oldest first. */
  approvals: ApprovalRequest[]
  /** Waiting `ask_user` questions of every branch, oldest first. */
  questions: PendingQuestion[]
  /** Waiting `exit_plan_mode` reviews of every branch, oldest first. */
  reviews: PendingPlan[]
  /** Whether plan mode is on for the main line. */
  planning: boolean
  /** The main line's task list. */
  todos: TodoItem[]
  /** The method names this Session serves (`session.methods`); empty when the daemon offers no table. */
  methods: readonly string[]
  /** The policy of `main`: read after history loads, then kept current by `permission/changed`. */
  permission: PermissionPolicy | undefined
  /** The main line's goal, folded from its events. */
  goal: Goal | undefined
  /** Messages queued on the main line while it runs, oldest first. */
  queued: readonly QueuedInput[]
  model: ModelId | undefined
  reasoningEffort: string | undefined
  /** Every line of the session, main first, in tree order. */
  lines: LineView[]
  /** Opened line transcripts, by branch id. */
  branches: Record<string, BranchView>
  lastTurnEnd: number | undefined
}

type BranchStream = {
  projection: Projection
  syncing: boolean
  done: boolean
  error: string
  /** Live events held while this line's history is paged in. */
  queued: SessionEvent[]
}

type Channel = {
  view: SessionView
  main: BranchStream
  branches: Map<string, BranchStream>
  lines: Lines
  states: Map<string, { running: boolean; stopping: boolean }>
  /** Set once history has applied: what arrives after this happens now and may alert the user. */
  live: boolean
  /** The questions already seen waiting, so each alerts once. */
  asked: Set<string>
  /** While `syncing`, every notification waits here so history applies first. */
  syncing: boolean
  queued: Notification[]
  historyQueue: string[]
  historyRunning: boolean
  publishTimer: ReturnType<typeof setTimeout> | undefined
}

const HISTORY_PAGE = 256
const HISTORY_TIMEOUT_MS = 60_000
/** Snapshots go out one frame after the first change, however fast events arrive. */
const PUBLISH_INTERVAL_MS = 16

function stream(): BranchStream {
  return { projection: new Projection(), syncing: false, done: false, error: "", queued: [] }
}

function idleState(): { running: boolean; stopping: boolean } {
  return { running: false, stopping: false }
}

/**
 * Attached sessions: each line's projection, running state and approvals. A
 * session stays attached for the life of the tab once opened, and every
 * attached session re-attaches and catches up after a reconnect.
 */
export const useChatStore = defineStore("chat", () => {
  const views = shallowReactive<Record<string, SessionView>>({})
  const channels = new Map<string, Channel>()
  let refreshTimer: ReturnType<typeof setTimeout> | undefined

  function client() {
    return useConnectionStore().client
  }

  const calls = sessionCalls({
    client,
    supports: (method) => useConnectionStore().supports(method),
    views,
    adoptModel(sessionId, model) {
      const channel = channels.get(sessionId)
      if (channel === undefined) return
      channel.main.projection.model = model
      publish(channel)
    },
  })
  const { sessionMethods, sessionPermission } = calls

  /** Attaches the session (once) and loads its main history. */
  function open(sessionId: string): void {
    if (channels.has(sessionId)) return
    const stored = useSessionsStore().byId.get(sessionId)
    const view = shallowReactive<SessionView>({
      sessionId,
      status: "loading",
      error: "",
      rows: [],
      running: stored?.running ?? false,
      stopping: false,
      sessionRunning: stored?.running ?? false,
      approvals: [],
      questions: [],
      reviews: [],
      planning: false,
      todos: [],
      methods: [],
      permission: undefined,
      goal: undefined,
      queued: [],
      model: stored?.model,
      reasoningEffort: undefined,
      lines: [],
      branches: {},
      lastTurnEnd: undefined,
    })
    const channel: Channel = {
      view,
      main: stream(),
      branches: new Map(),
      lines: new Lines(),
      states: new Map(),
      syncing: false,
      live: false,
      asked: new Set(),
      queued: [],
      historyQueue: [],
      historyRunning: false,
      publishTimer: undefined,
    }
    channels.set(sessionId, channel)
    views[sessionId] = view
    void sync(channel)
  }

  /** After a reconnect: attach again and fetch every line's history after its last applied event. */
  function resync(): void {
    for (const channel of channels.values()) {
      if (channel.view.status !== "closed") void sync(channel)
    }
  }

  async function sync(channel: Channel): Promise<void> {
    if (channel.syncing) return
    const { view } = channel
    channel.syncing = true
    channel.queued = []
    view.approvals = []
    channel.main.syncing = true
    channel.main.done = false
    channel.main.error = ""
    channel.main.queued = []
    for (const [id, branch] of channel.branches) {
      branch.syncing = true
      branch.done = false
      branch.error = ""
      branch.queued = []
      if (!channel.historyQueue.includes(id)) channel.historyQueue.push(id)
    }
    try {
      await client().call("session.attach", { sessionId: view.sessionId }, readAck)
      view.methods = await sessionMethods(view.sessionId)
      const main = await loadHistory(channel, MAIN_BRANCH)
      if (main !== "ok") {
        if (main === "failed" && view.status !== "closed") {
          view.status = "failed"
          view.error = channel.main.error
        }
        return
      }
      const queued = channel.queued
      channel.queued = []
      channel.syncing = false
      for (const notification of queued) receive(notification)
      if (view.status !== "closed") view.permission = await sessionPermission(view.sessionId)
      if (view.status !== "closed") {
        view.status = "ready"
        view.error = ""
      }
      publish(channel)
      if (view.status !== "closed") void pumpHistory(channel)
    } catch (error) {
      if (error instanceof RpcError && error.appError === "session_closed") view.status = "closed"
      else if (!(error instanceof ConnectionLost)) {
        view.status = "failed"
        view.error = error instanceof Error ? error.message : String(error)
      }
    } finally {
      channel.syncing = false
      channel.main.syncing = false
      channel.queued = []
      publish(channel)
      channel.live = true
    }
  }

  function alertUser(channel: Channel, key: TranslationKey): void {
    if (!channel.live || !inDesktopShell) return
    const session = useSessionsStore().byId.get(channel.view.sessionId)
    notifyDesktop(translate(key), session?.title ?? session?.preview ?? translate("sidebar.untitled"))
  }

  function receive(notification: Notification): void {
    const sessionId = notification.method === "session.model.progress"
      ? notification.progress.sessionId
      : notification.method === "approval.request" ? notification.request.sessionId : notification.sessionId
    const channel = channels.get(sessionId)
    if (channel === undefined) return
    if (channel.syncing) {
      channel.queued.push(notification)
      return
    }
    applyNotification(channel, notification)
  }

  function applyNotification(channel: Channel, notification: Notification): void {
    const view = channel.view
    switch (notification.method) {
      case "session.event": {
        const branchId = notification.branchId
        const stream = ensureBranch(channel, branchId)
        if (stream.syncing) {
          channel.lines.apply(branchId, notification.event)
          stream.queued.push(notification.event)
        } else {
          applyEvent(channel, branchId, notification.event)
        }
        break
      }
      case "session.model.progress": {
        const branchId = notification.progress.branchId
        const stream = ensureBranch(channel, branchId)
        if (stream.syncing) break
        stream.projection.progress(notification.progress)
        changed(channel)
        break
      }
      case "session.state": {
        if (notification.branchId === MAIN_BRANCH) {
          if (view.running && !notification.running) {
            refreshDirectorySoon()
            alertUser(channel, "desktop.runFinished")
            useUiStore().markUnread(view.sessionId)
          }
        }
        channel.states.set(notification.branchId, { running: notification.running, stopping: notification.stopping })
        channel.lines.setRunning(notification.branchId, notification.running)
        if (notification.branchId !== MAIN_BRANCH) ensureBranch(channel, notification.branchId)
        const running = anyRunning(channel)
        view.running = stateOf(channel, MAIN_BRANCH).running
        view.stopping = stateOf(channel, MAIN_BRANCH).stopping
        view.sessionRunning = running
        useSessionsStore().setRunning(view.sessionId, running)
        changed(channel)
        break
      }
      case "approval.request":
        if (!view.approvals.some((approval) => approval.requestId === notification.request.requestId && approval.branchId === notification.request.branchId)) {
          view.approvals = [...view.approvals, notification.request]
          alertUser(channel, "desktop.needsApproval")
        }
        break
      case "approval.resolved":
      case "approval.cancelled":
        if (view.approvals.some((approval) => approval.branchId === notification.branchId && approval.requestId === notification.requestId)) removeApproval(view, notification.branchId, notification.requestId)
        break
      case "memberApprovalEnded": {
        const ended = view.approvals.filter((approval) => approval.branchId === notification.branchId && approval.call.name === "member_approval_allow" && approval.call.input.approvalId === notification.approvalId)
        for (const approval of ended) {
          removeApproval(view, approval.branchId, approval.requestId)
          void respond(view.sessionId, approval, "deny").catch(() => {})
        }
        break
      }
      case "session.closed":
        view.status = "closed"
        view.running = false
        view.stopping = false
        view.sessionRunning = false
        view.approvals = []
        view.questions = []
        view.reviews = []
        useSessionsStore().forget(view.sessionId)
        useUiStore().resetSessionTabs(view.sessionId)
        break
    }
  }

  function applyEvent(channel: Channel, branchId: string, event: SessionEvent): void {
    channel.lines.apply(branchId, event)
    const stream = ensureBranch(channel, branchId)
    if (!stream.projection.apply(event)) return
    if (event.content.kind === "branch-create") ensureBranch(channel, event.content.branchId)
    if (branchId === MAIN_BRANCH && event.content.kind === "permission-changed") channel.view.permission = event.content.policy
    changed(channel)
  }

  function ensureBranch(channel: Channel, branchId: string): BranchStream {
    if (branchId === MAIN_BRANCH) return channel.main
    let branch = channel.branches.get(branchId)
    if (branch === undefined) {
      branch = stream()
      branch.syncing = true
      channel.branches.set(branchId, branch)
      if (!channel.historyQueue.includes(branchId)) channel.historyQueue.push(branchId)
      if (!channel.syncing) void pumpHistory(channel)
      changed(channel)
    }
    return branch
  }

  /** Loads one line's history from its cursor, then applies the events held while it paged. */
  async function loadHistory(channel: Channel, branchId: string): Promise<"ok" | "lost" | "failed"> {
    const branch = branchId === MAIN_BRANCH ? channel.main : channel.branches.get(branchId)
    if (branch === undefined) return "failed"
    branch.syncing = true
    branch.error = ""
    branch.done = false
    let after = branch.projection.cursor
    let through: number | undefined
    for (let attempt = 0; ; attempt++) {
      try {
        for (;;) {
          const page = await client().call("session.history", {
            sessionId: channel.view.sessionId,
            branchId,
            after,
            limit: HISTORY_PAGE,
            ...(through === undefined ? {} : { through }),
          }, readHistory, HISTORY_TIMEOUT_MS)
          if ((through !== undefined && page.through !== through) || page.nextAfter < after || page.nextAfter > page.through || (!page.done && page.nextAfter === after)) {
            throw new ProtocolError("Invalid history page cursor")
          }
          through = page.through
          for (const event of page.events) {
            channel.lines.apply(branchId, event)
            branch.projection.apply(event)
            if (event.content.kind === "branch-create") ensureBranch(channel, event.content.branchId)
          }
          after = page.nextAfter
          if (page.done) break
        }
        const queued = branch.queued
        branch.queued = []
        for (const event of queued) applyEvent(channel, branchId, event)
        branch.done = true
        branch.syncing = false
        changed(channel)
        return "ok"
      } catch (error) {
        if (branchId !== MAIN_BRANCH && attempt < 10 && transientBranch(error)) {
          await new Promise((resolve) => setTimeout(resolve, 100))
          continue
        }
        branch.syncing = false
        if (error instanceof ConnectionLost) {
          branch.done = false
          branch.error = ""
          changed(channel)
          return "lost"
        }
        branch.done = true
        branch.error = error instanceof Error ? error.message : String(error)
        if (error instanceof RpcError && error.appError === "session_closed") channel.view.status = "closed"
        changed(channel)
        return "failed"
      }
    }
  }

  /** A line is published just after `branch/create`; its first history read can lose the race. */
  function transientBranch(error: unknown): boolean {
    return error instanceof RpcError
      && (error.appError === "branch_missing" || error.appError === "unknown_branch" || error.message.startsWith("unknown branch"))
  }

  async function pumpHistory(channel: Channel): Promise<void> {
    if (channel.historyRunning || channel.syncing) return
    channel.historyRunning = true
    try {
      while (channel.historyQueue.length > 0) {
        const branchId = channel.historyQueue.shift()!
        if (branchId === MAIN_BRANCH || !channel.branches.has(branchId)) continue
        await loadHistory(channel, branchId)
      }
    } finally {
      channel.historyRunning = false
      publish(channel)
    }
  }

  function retryLine(sessionId: string, branchId: string): void {
    const channel = channels.get(sessionId)
    if (channel === undefined || branchId === MAIN_BRANCH) return
    const branch = channel.branches.get(branchId)
    if (branch === undefined) return
    branch.error = ""
    branch.done = false
    branch.syncing = true
    if (!channel.historyQueue.includes(branchId)) channel.historyQueue.push(branchId)
    publish(channel)
    void pumpHistory(channel)
  }

  function stateOf(channel: Channel, branchId: string): { running: boolean; stopping: boolean } {
    return channel.states.get(branchId) ?? idleState()
  }

  function anyRunning(channel: Channel): boolean {
    if (stateOf(channel, MAIN_BRANCH).running) return true
    for (const id of channel.branches.keys()) {
      if (stateOf(channel, id).running) return true
    }
    return false
  }

  function changed(channel: Channel): void {
    channel.publishTimer ??= setTimeout(() => publish(channel), PUBLISH_INTERVAL_MS)
  }

  function publish(channel: Channel): void {
    clearTimeout(channel.publishTimer)
    channel.publishTimer = undefined
    const view = channel.view
    const main = stateOf(channel, MAIN_BRANCH)
    view.rows = channel.main.projection.rows.slice()
    view.lastTurnEnd = channel.main.projection.lastTurnEnd
    view.running = main.running
    view.stopping = main.stopping
    view.sessionRunning = anyRunning(channel)
    view.model = channel.main.projection.model ?? useSessionsStore().byId.get(view.sessionId)?.model
    view.reasoningEffort = channel.main.projection.reasoningEffort
    view.lines = channel.lines.views()
    view.goal = channel.main.projection.goal
    view.queued = channel.main.projection.queued()
    view.questions = [
      ...channel.main.projection.waiting().map((ask) => ({ branchId: MAIN_BRANCH, ...ask })),
      ...[...channel.branches].flatMap(([id, branch]) => branch.projection.waiting().map((ask) => ({ branchId: id, ...ask }))),
    ]
    const asked = view.questions.filter((ask) => !channel.asked.has(ask.requestId))
    for (const ask of asked) channel.asked.add(ask.requestId)
    if (asked.length > 0) alertUser(channel, "desktop.needsAnswer")
    view.reviews = [
      ...channel.main.projection.reviews().map((review) => ({ branchId: MAIN_BRANCH, ...review })),
      ...[...channel.branches].flatMap(([id, branch]) => branch.projection.reviews().map((review) => ({ branchId: id, ...review }))),
    ]
    const reviewed = view.reviews.filter((review) => !channel.asked.has(`plan:${review.requestId}:${review.callId}`))
    for (const review of reviewed) channel.asked.add(`plan:${review.requestId}:${review.callId}`)
    if (reviewed.length > 0) alertUser(channel, "desktop.needsPlanReview")
    view.planning = channel.main.projection.planningFold.planning
    view.todos = channel.main.projection.planningFold.todos()
    const branches: Record<string, BranchView> = {}
    for (const [id, branch] of channel.branches) {
      const state = stateOf(channel, id)
      branches[id] = {
        rows: branch.projection.rows.slice(),
        running: state.running,
        stopping: state.stopping,
        status: branch.error !== "" ? "failed" : branch.done ? "ready" : "loading",
        error: branch.error,
        lastTurnEnd: branch.projection.lastTurnEnd,
      }
    }
    view.branches = branches
  }

  /** A finished turn changes the directory's preview and activity time. */
  function refreshDirectorySoon(): void {
    clearTimeout(refreshTimer)
    refreshTimer = setTimeout(() => void useSessionsStore().refresh(), 500)
  }

  function ensureLine(sessionId: string, branchId: string): void {
    const channel = channels.get(sessionId)
    if (channel === undefined) return
    if (branchId !== MAIN_BRANCH && !channel.lines.has(branchId)) return
    ensureBranch(channel, branchId)
    publish(channel)
  }


  const { submit, stop, createLine, endLine, respond, allowAlways, allowForSession, answerQuestion, setPlanning, reviewPlan, compact, usage, fork, writeGoal, setEffort, setPermission, withdrawInput, searchFiles, turnChanges, rewindTurn, memberApproval, setModel } = calls
  return { views, open, resync, receive, ensureLine, retryLine, submit, stop, createLine, endLine, respond, allowAlways, allowForSession, answerQuestion, setPlanning, reviewPlan, compact, usage, fork, writeGoal, setEffort, setPermission, withdrawInput, searchFiles, turnChanges, rewindTurn, memberApproval, setModel }
})
