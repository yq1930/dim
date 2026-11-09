/**
 * Folds one branch's events into the timeline rows the chat page draws,
 * following the TUI's `crates/dim-cli/src/projection.rs`:
 *
 * - events apply once, in sequence order; a replayed or duplicate one is skipped;
 * - reasoning and answer text stream into one row per model request, and the
 *   unwritten tail from `session.model.progress` shows until committed deltas
 *   overtake it; a stream settles on what the Timeline holds;
 * - a tool row opens on `step/tool-use` and completes on its result.
 *
 * Rows are immutable values: a change replaces the row, so a published copy of
 * `rows` re-renders only the rows that changed.
 */
import { GoalFold, type Goal } from "./goal"
import { PlanningFold } from "./planning"
import type { AskQuestion, ExecStatus, MemberOutcome, ModelId, ModelProgress, QueuedInput, SessionEvent, ToolResult } from "./rpc/protocol"

export type BackgroundTask = {
  taskId: string
  label: string
  startedAt: number
  finish?: { at: number; status: ExecStatus; result: ToolResult }
}

/** Work this line handed to a team member (`collaboration/request`), and how it ended. */
export type MemberTask = {
  /** The request event's id. */
  requestId: string
  /** The member's own Session, once delivery names it. */
  sessionId?: string
  prompt: string
  startedAt: number
  finish?: { at: number; status: MemberOutcome; text: string }
}

export type TimelineRow =
  | { kind: "user"; key: string; text: string; skills?: string[]; at: number; sequence: number; cut: number }
  | { kind: "assistant"; key: string; text: string; streaming: boolean }
  | { kind: "thinking"; key: string; text: string; streaming: boolean; elapsedMs?: number }
  | { kind: "tool"; key: string; callId: string; name: string; input: Record<string, unknown>; pending: boolean; result?: ToolResult }
  | { kind: "line"; key: string; branchId: string; purpose: string; before?: string; failed?: boolean }
  | { kind: "line-ended"; key: string; issuer: "itself" | "operation" | "ancestor"; ancestor?: string }
  | { kind: "source-message"; key: string; from: string; steer: boolean; final: boolean; body: string }
  | { kind: "source-idle"; key: string; branch: string; status: string }
  | { kind: "source-ended"; key: string; branch: string }
  | { kind: "error"; key: string; text: string }
  | { kind: "stopped"; key: string }
  /** A turn that completed, or that wrote files: how long it took and how many files it wrote. */
  | { kind: "turn-summary"; key: string; turnId: string; outcome: "completed" | "failed" | "cancelled"; files: number; durationMs?: number }
  | { kind: "files-rewound"; key: string; count: number }
  /** A member's result came back: the request it answers, by the task's first line. */
  | { kind: "member-result"; key: string; requestId: string; task: string; status: MemberOutcome }

type UserRow = Extract<TimelineRow, { kind: "user" }>

/** A request's reasoning or answer while it is still arriving. */
type Stream = {
  index: number
  /** The request's committed deltas so far. */
  written: string
  /** Unicode scalar values in `written`, the unit of progress offsets. */
  writtenChars: number
  /** The latest unwritten tail and the offset it follows. */
  progress?: { text: string; offset: number }
}

export class Projection {
  rows: TimelineRow[] = []
  readonly backgroundTasks = new Map<string, BackgroundTask>()
  /** Work handed to team members, by request event id. */
  readonly members = new Map<string, MemberTask>()
  /** Highest event sequence applied. */
  cursor = 0
  /** The newest turn end on this line, the cut `/branch` and `+` use. */
  lastTurnEnd: number | undefined
  /** The model the branch last adopted (`context/configured`). */
  model: ModelId | undefined
  reasoningEffort: string | undefined
  private answers = new Map<string, Stream>()
  private thoughts = new Map<string, Stream>()
  private requestStarts = new Map<string, number>()
  private tools = new Map<string, number>()
  /** The files `write` and `edit` changed in the turn under way. */
  private turnWrites = new Set<string>()
  /**
   * The turn under way and when its `turn/start` came. Inputs mark no start: an
   * appended one, such as the client snapshot written on attach, opens no turn.
   */
  private turnStart: { turnId: string; at: number } | undefined
  private lines = new Map<string, number>()
  /** `ask_user` calls whose batch has no `step/tool-result` yet, oldest first. */
  private asks: Array<{ requestId: string; callId: string; questions: AskQuestion[] }> = []
  /**
   * Messages the server holds in the queue: written, not yet adopted, not withdrawn.
   * Each enters the transcript at its `input/applied`, as the TUI's does, so the
   * transcript keeps the order the model reads and a withdrawn one never shows.
   */
  private held = new Map<string, { input: QueuedInput; row: UserRow }>()
  /** The branch's goal, folded from its events (goal.ts). */
  readonly goalFold = new GoalFold()
  /** The branch's task list and plan mode, folded from its events (planning.ts). */
  readonly planningFold = new PlanningFold()
  /** `exit_plan_mode` calls whose batch has no `step/tool-result` yet, oldest first. */
  private plans: Array<{ requestId: string; callId: string; plan: string }> = []

  /** The goal in force on this branch. */
  get goal(): Goal | undefined {
    return this.goalFold.goal()
  }

  /** When the turn under way started, by its `turn/start`. */
  get turnStartedAt(): number | undefined {
    return this.turnStart?.at
  }

  /** The messages still queued on this branch, oldest first. */
  queued(): readonly QueuedInput[] {
    return [...this.held.values()].map((entry) => entry.input)
  }

  /** The waiting `ask_user` calls of this branch: named by batch `requestId` and call id. */
  waiting(): readonly { requestId: string; callId: string; questions: AskQuestion[] }[] {
    return this.asks
  }

  /** The waiting `exit_plan_mode` calls of this branch: named by batch `requestId` and call id. */
  reviews(): readonly { requestId: string; callId: string; plan: string }[] {
    return this.plans
  }

  /** Applies one event; false when it was already applied. */
  apply(event: SessionEvent): boolean {
    if (event.sequence <= this.cursor) return false
    this.cursor = event.sequence
    this.goalFold.apply(event)
    this.planningFold.apply(event)
    const content = event.content
    switch (content.kind) {
      case "exec-started": {
        const row = this.rows.find((item) => item.kind === "tool" && item.callId === content.callId)
        const command = row?.kind === "tool" ? row.input.command : undefined
        this.backgroundTasks.set(content.taskId, {
          taskId: content.taskId,
          label: typeof command === "string" && command !== "" ? command : content.description,
          startedAt: event.createAt,
        })
        break
      }
      case "exec-ended": {
        const task = this.backgroundTasks.get(content.taskId)
        if (task) this.backgroundTasks.set(content.taskId, { ...task, finish: { at: event.createAt, status: content.status, result: content.result } })
        break
      }
      case "member-request":
        this.members.set(event.id, { requestId: event.id, prompt: content.prompt, startedAt: event.createAt, ...(content.sessionId === undefined ? {} : { sessionId: content.sessionId }) })
        break
      case "member-delivered": {
        // Deliveries of a cancel, close or message name their own operation, not a request.
        const task = this.members.get(content.opEventId)
        if (task === undefined || task.finish !== undefined) break
        this.members.set(content.opEventId, content.error === undefined
          ? { ...task, sessionId: content.sessionId }
          : { ...task, finish: { at: event.createAt, status: "failed", text: content.error } })
        break
      }
      case "member-result": {
        const task = this.members.get(content.requestEventId)
        const text = content.reason === undefined ? content.text : `${content.reason}\n${content.text}`.trim()
        if (task !== undefined) this.members.set(content.requestEventId, { ...task, sessionId: content.sessionId, finish: { at: event.createAt, status: content.status, text } })
        this.push({ kind: "member-result", key: `member-result-${event.sequence}`, requestId: content.requestEventId, task: firstLine(task?.prompt ?? ""), status: content.status })
        break
      }
      case "input":
        if (content.user !== undefined) {
          const text = content.text ?? ""
          this.held.set(event.id, {
            input: { id: event.id, text, skills: content.skills, user: content.user },
            row: {
              kind: "user",
              key: `user-${event.sequence}`,
              text,
              skills: content.skills,
              at: event.createAt,
              sequence: event.sequence,
              cut: Math.max(0, event.sequence - 1),
            },
          })
        }
        break
      case "input-applied": {
        const entry = this.held.get(content.inputId)
        this.held.delete(content.inputId)
        if (entry !== undefined) this.push(entry.row)
        break
      }
      case "input-deleted":
        this.held.delete(content.inputId)
        break
      case "source-input": {
        const notice = branchNotice(content.text, content.mode) ?? rewindNotice(content.text)
        if (notice !== undefined) this.push({ ...notice, key: `notice-${event.sequence}` })
        break
      }
      case "branch-create": {
        const at = content.at ?? event.sequence
        const message = this.rows.find((row) => row.kind === "user" && row.cut === at)
        const before = message?.kind === "user" ? clipFirstLine(message.text) : undefined
        this.lines.set(content.branchId, this.push({
          kind: "line",
          key: `line-${event.sequence}`,
          branchId: content.branchId,
          purpose: firstLine(content.prompt),
          ...(before === undefined ? {} : { before }),
        }))
        break
      }
      case "branch-create-failed": {
        const index = this.lines.get(content.branchId)
        if (index !== undefined) this.update(index, { failed: true })
        break
      }
      case "branch-end": {
        const ancestor = content.ancestor
        this.push({
          kind: "line-ended",
          key: `line-ended-${event.sequence}`,
          issuer: ancestor !== undefined ? "ancestor" : content.source.kind === "tool" ? "itself" : "operation",
          ...(ancestor === undefined ? {} : { ancestor }),
        })
        break
      }
      case "model-start":
        this.requestStarts.set(content.requestId, event.createAt)
        break
      case "thinking": {
        const thought = this.openThinking(content.requestId)
        commit(thought, content.delta)
        const start = this.requestStarts.get(content.requestId)
        this.update(thought.index, { text: shown(thought), ...(start === undefined ? {} : { elapsedMs: Math.max(0, event.createAt - start) }) })
        break
      }
      case "text": {
        this.closeThinking(content.requestId)
        const answer = this.openAnswer(content.requestId)
        commit(answer, content.delta)
        this.update(answer.index, { text: shown(answer) })
        break
      }
      case "tool-use":
        this.closeThinking(content.requestId)
        this.closeAnswer(content.requestId)
        this.requestStarts.delete(content.requestId)
        this.tools.set(content.callId, this.push({ kind: "tool", key: `tool-${content.callId}`, callId: content.callId, name: content.name, input: content.input, pending: true }))
        break
      case "tool-call":
        for (const ask of content.asks) this.asks.push({ requestId: content.requestId, callId: ask.callId, questions: ask.questions })
        for (const plan of content.plans) this.plans.push({ requestId: content.requestId, callId: plan.callId, plan: plan.plan })
        break
      case "tool-completed":
        this.completeTool(content.callId, content.result)
        break
      case "tool-result":
        for (const result of content.results) this.completeTool(result.callId, result.result)
        this.asks = this.asks.filter((ask) => ask.requestId !== content.requestId || !content.results.some((result) => result.callId === ask.callId))
        this.plans = this.plans.filter((plan) => plan.requestId !== content.requestId || !content.results.some((result) => result.callId === plan.callId))
        break
      case "model-end":
        this.closeThinking(content.requestId)
        this.closeAnswer(content.requestId)
        this.requestStarts.delete(content.requestId)
        if (content.error !== undefined) this.push({ kind: "error", key: `error-${event.sequence}`, text: content.error })
        break
      case "turn-start":
        this.turnStart = { turnId: content.turnId, at: event.createAt }
        break
      case "turn-end": {
        const start = this.turnStart
        const startedAt = start !== undefined && start.turnId === content.turnId ? start.at : undefined
        this.lastTurnEnd = event.sequence
        for (const requestId of [...this.thoughts.keys()]) this.closeThinking(requestId)
        for (const requestId of [...this.answers.keys()]) this.closeAnswer(requestId)
        this.requestStarts.clear()
        if (content.outcome === "failed") this.push({ kind: "error", key: `error-${event.sequence}`, text: content.error ?? "" })
        if (content.outcome === "cancelled") this.push({ kind: "stopped", key: `stopped-${event.sequence}` })
        if (content.outcome === "completed" || this.turnWrites.size > 0) {
          this.push({
            kind: "turn-summary",
            key: `turn-summary-${event.sequence}`,
            turnId: content.turnId,
            outcome: content.outcome,
            files: this.turnWrites.size,
            ...(startedAt === undefined ? {} : { durationMs: event.createAt - startedAt }),
          })
        }
        this.turnWrites.clear()
        this.turnStart = undefined
        break
      }
      case "context-configured":
        this.model = content.model
        this.reasoningEffort = content.reasoningEffort
        break
      case "other":
        break
    }
    return true
  }

  /** Shows a request's output that is not written yet. */
  progress(progress: ModelProgress): void {
    const requestId = progress.requestId
    if (requestId === undefined) return
    if (progress.thinking !== "") {
      const thought = this.openThinking(requestId)
      thought.progress = { text: progress.thinking, offset: progress.thinkingOffset }
      this.update(thought.index, { text: shown(thought) })
    }
    if (progress.text !== "") {
      const answer = this.openAnswer(requestId)
      answer.progress = { text: progress.text, offset: progress.textOffset }
      this.update(answer.index, { text: shown(answer) })
    }
  }

  private push(row: TimelineRow): number {
    this.rows.push(row)
    return this.rows.length - 1
  }

  private update(index: number, patch: Partial<TimelineRow>): void {
    this.rows[index] = { ...this.rows[index]!, ...patch } as TimelineRow
  }

  private openThinking(requestId: string): Stream {
    let thought = this.thoughts.get(requestId)
    if (thought === undefined) {
      thought = { index: this.push({ kind: "thinking", key: `thinking-${requestId}-${this.rows.length}`, text: "", streaming: true }), written: "", writtenChars: 0 }
      this.thoughts.set(requestId, thought)
    }
    return thought
  }

  private openAnswer(requestId: string): Stream {
    let answer = this.answers.get(requestId)
    if (answer === undefined) {
      answer = { index: this.push({ kind: "assistant", key: `assistant-${requestId}-${this.rows.length}`, text: "", streaming: true }), written: "", writtenChars: 0 }
      this.answers.set(requestId, answer)
    }
    return answer
  }

  /** A thought settles when the request moves on: progress never written (an interrupted thought) goes. */
  private closeThinking(requestId: string): void {
    const thought = this.thoughts.get(requestId)
    if (thought === undefined) return
    this.thoughts.delete(requestId)
    this.update(thought.index, { text: thought.written, streaming: false })
  }

  private closeAnswer(requestId: string): void {
    const answer = this.answers.get(requestId)
    if (answer === undefined) return
    this.answers.delete(requestId)
    this.update(answer.index, { text: answer.written, streaming: false })
  }

  private completeTool(callId: string, result: ToolResult): void {
    const index = this.tools.get(callId)
    if (index === undefined) return
    this.tools.delete(callId)
    const row = this.rows[index]!
    if (row.kind === "tool" && (row.name === "write" || row.name === "edit")) {
      for (const path of result.filesWritten ?? []) this.turnWrites.add(path)
    }
    this.update(index, { pending: false, result })
  }
}

type BranchNotice =
  | { kind: "source-message"; from: string; steer: boolean; final: boolean; body: string }
  | { kind: "source-idle"; branch: string; status: string }
  | { kind: "source-ended"; branch: string }

/** The record of an undone turn: one line per restored or removed file. */
function rewindNotice(text: string): { kind: "files-rewound"; count: number } | undefined {
  if (!text.startsWith("<dim-file-rewind>")) return undefined
  const lines = elementBody(text, "</dim-file-rewind>").split("\n").filter((line) => line.trim() !== "")
  return { kind: "files-rewound", count: lines.length }
}

function branchNotice(text: string, mode: string | undefined): BranchNotice | undefined {
  if (text.startsWith("<dim-branch-message")) {
    const from = attr(text, "from")
    if (from === undefined) return undefined
    return {
      kind: "source-message",
      from,
      steer: mode === "steer",
      final: attr(text, "final") !== undefined,
      body: oneLine(unescape(elementBody(text, "</dim-branch-message>"))),
    }
  }
  if (text.startsWith("<dim-branch-idle")) {
    const branch = attr(text, "branch")
    if (branch === undefined) return undefined
    return { kind: "source-idle", branch, status: attr(text, "status") ?? "idle" }
  }
  if (text.startsWith("<dim-branch-ended")) {
    const branch = attr(text, "branch")
    return branch === undefined ? undefined : { kind: "source-ended", branch }
  }
  return undefined
}

function firstLine(text: string): string {
  return text.split("\n")[0]?.trim() ?? ""
}

function clipFirstLine(text: string): string {
  const line = firstLine(text)
  const characters = [...line]
  return characters.length <= 40 ? line : `${characters.slice(0, 40).join("")}…`
}

function oneLine(text: string): string {
  return text.split(/\s+/u).filter(Boolean).join(" ")
}

function unescape(text: string): string {
  return text.replaceAll("&lt;", "<").replaceAll("&gt;", ">").replaceAll("&amp;", "&")
}

function attr(text: string, name: string): string | undefined {
  const needle = `${name}="`
  const start = text.indexOf(needle)
  if (start < 0) return undefined
  const from = start + needle.length
  const end = text.indexOf("\"", from)
  return end < 0 ? undefined : text.slice(from, end)
}

function elementBody(text: string, close: string): string {
  const newline = text.indexOf("\n")
  const body = newline < 0 ? "" : text.slice(newline + 1)
  const end = body.indexOf(close)
  return end < 0 ? body : body.slice(0, end)
}

function commit(stream: Stream, delta: string): void {
  stream.written += delta
  stream.writtenChars += [...delta].length
}

/**
 * Committed text plus the part of the latest progress it has not overtaken.
 * Progress that runs ahead of the committed deltas waits for the next notice.
 */
function shown(stream: Stream): string {
  const progress = stream.progress
  if (progress === undefined) return stream.written
  const overlap = stream.writtenChars - progress.offset
  if (overlap < 0) return stream.written
  const tail = [...progress.text]
  if (overlap >= tail.length) {
    stream.progress = undefined
    return stream.written
  }
  return stream.written + tail.slice(overlap).join("")
}
