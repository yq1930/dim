import { alwaysCommand, alwaysRule, fileGrant, sameRule } from "../approvals"
import { RpcError } from "../rpc/client"
import type { GoalCall } from "../goal"
import { MAIN_BRANCH, type ApprovalDecision, type ApprovalRequest, type DraftAttachments, type FileCandidate, type ModelId, type PendingPlan, type PendingQuestion, type PermissionPolicy, type PermissionPreset, type QuestionAnswer, type RewindResult, type TurnChanges, type UserInput, type WithdrawResult } from "../rpc/protocol"
import { readRevision, readRules } from "../rpc/general-settings"
import { readAck, readAttachmentPath, readBranchCreate, readCompactResult, readConfigurationOutcome, readContextUsage, readExecOutput, readFileCandidates, readForkedSessionId, readMemberApproval, readModelUpdate, readPermissionState, readPermissionVersion, readRewindResult, readSessionMethods, readTurnChanges, readWithdrawResult } from "../rpc/read"
import type { PermissionOutcome, SessionView } from "./chat"

/** The part of the connection's client the calls use. */
type Caller = { call<T>(method: string, params: Record<string, unknown>, read: (value: unknown) => T): Promise<T> }

export function removeApproval(view: SessionView, branchId: string, requestId: string): void {
  view.approvals = view.approvals.filter((approval) => approval.branchId !== branchId || approval.requestId !== requestId)
}

/**
 * The calls the web workspace makes on a Session, beside the live state the chat
 * store keeps: each wraps one method, or a short sequence such as saving a rule
 * and then allowing. A reply changes `views` only where it settles something the
 * Session's own events would not; `adoptModel` hands a model change back to the
 * line it was made on. Dependencies come in, so this module reaches no store.
 */
export function sessionCalls(deps: {
  client: () => Caller
  supports: (method: string) => boolean
  views: Record<string, SessionView>
  adoptModel: (sessionId: string, model: ModelId) => void
}) {
  const { client, supports, views, adoptModel } = deps

  /** `queue` waits for the turn under way to end; `steer` enters it at its next tool result. */
  async function submit(sessionId: string, branchId: string, text: string, attachments: DraftAttachments = { images: [], pastes: [] }, mode: "queue" | "steer" = "queue"): Promise<void> {
    let body = text
    for (const paste of attachments.pastes) {
      const path = await client().call("attachment.put", { sessionId, text: paste }, readAttachmentPath)
      body = `${body}${body === "" ? "" : "\n"}[Pasted text: ${path} (${paste.split("\n").length} lines)]`
    }
    for (const file of attachments.files ?? []) {
      const path = await client().call("attachment.put", file.pdfBase64 === undefined ? { sessionId, text: file.text } : { sessionId, pdfBase64: file.pdfBase64 }, readAttachmentPath)
      const name = file.name.replace(/([\\`*_\[\]<>!])/g, "\\$1").replace(/[\r\n]/g, " ")
      const size = file.pdfBase64 === undefined ? `${file.text.split("\n").length} lines` : "PDF"
      body = `${body}${body === "" ? "" : "\n"}[File: ${name}; ${path} (${size})]`
    }
    const params: Record<string, unknown> = { sessionId, branchId, submissionId: crypto.randomUUID(), mode }
    if (attachments.images.length === 0) params.text = body
    else params.user = { content: [...(body === "" ? [] : [{ type: "text", text: body }]), ...attachments.images] }
    await client().call("session.submit", params, readAck)
  }

  async function stop(sessionId: string, branchId: string): Promise<void> {
    await client().call("session.stop", { sessionId, branchId, reason: "Stopped from the web workspace" }, readAck)
  }

  function taskOutput(sessionId: string, branchId: string, taskId: string): Promise<string> {
    return client().call("session.exec.output", { sessionId, branchId, taskId }, readExecOutput)
  }

  async function stopTask(sessionId: string, branchId: string, taskId: string): Promise<void> {
    await client().call("session.exec.stop", { sessionId, branchId, taskId }, readAck)
  }

  async function createLine(sessionId: string, parentBranchId: string, at: number, prompt: string): Promise<string> {
    const opened = await client().call("session.branch_create", {
      sessionId,
      parentBranchId,
      at,
      prompt,
    }, readBranchCreate)
    return opened.branchId
  }

  async function endLine(sessionId: string, branchId: string): Promise<void> {
    await client().call("session.branch_end", { sessionId, branchId }, readAck)
  }

  async function memberApproval(sessionId: string, approval: ApprovalRequest) {
    return client().call("session.collaboration.approval.get", { sessionId, branchId: approval.branchId, requestId: approval.requestId }, readMemberApproval)
  }

  async function respond(sessionId: string, approval: ApprovalRequest, decision: ApprovalDecision): Promise<void> {
    await client().call("approval.respond", {
      sessionId,
      branchId: approval.branchId,
      requestId: approval.requestId,
      decision,
    }, readAck)
    const view = views[sessionId]
    if (view !== undefined) removeApproval(view, approval.branchId, approval.requestId)
  }

  /** Allows the approval and, for a simple command, saves an exact allow rule first. */
  async function allowAlways(sessionId: string, approval: ApprovalRequest): Promise<void> {
    const command = alwaysCommand(approval)
    if (command === undefined) throw new Error("This approval has no simple command to allow")
    const rule = alwaysRule(command)
    const rules = await client().call("permission.rules", {}, readRules)
    if (!rules.commandRules.some((existing) => sameRule(existing, rule))) {
      await client().call("permission.set_rules", { expectedRevision: rules.revision, commandRules: [...rules.commandRules, rule] }, readRevision)
    }
    await respond(sessionId, approval, "allow")
  }

  /** Saves a session grant for the approval's file, then allows it. */
  async function allowForSession(sessionId: string, approval: ApprovalRequest): Promise<void> {
    const grant = fileGrant(approval)
    if (grant === undefined) throw new Error("This approval names no file to allow")
    await client().call("session.grants.add", { sessionId, ...grant }, readAck)
    await respond(sessionId, approval, "allow")
  }

  /** Answers a waiting question; `undefined` declines it. The prompt closes when the batch's result arrives. */
  async function answerQuestion(sessionId: string, question: PendingQuestion, answers: QuestionAnswer[] | undefined): Promise<void> {
    await client().call("session.question.respond", {
      sessionId,
      branchId: question.branchId,
      requestId: question.requestId,
      callId: question.callId,
      ...(answers === undefined ? { declined: true } : { answers }),
    }, readAck)
  }

  /** Turns plan mode on or off for the main line (Planning §2). */
  async function setPlanning(sessionId: string, active: boolean): Promise<void> {
    await client().call("session.plan.set", { sessionId, branchId: MAIN_BRANCH, active }, readAck)
  }

  /** Approves a waiting plan, or sends it back with the user's feedback. */
  async function reviewPlan(sessionId: string, review: PendingPlan, approved: boolean, feedback?: string): Promise<void> {
    await client().call("session.plan.respond", {
      sessionId,
      branchId: review.branchId,
      requestId: review.requestId,
      callId: review.callId,
      approved,
      ...(approved || feedback === undefined || feedback.trim() === "" ? {} : { feedback: feedback.trim() }),
    }, readAck)
  }

  /** The Session's method table; a daemon without it offers no session commands, and an unreadable table offers none either. */
  async function sessionMethods(sessionId: string): Promise<string[]> {
    if (!supports("session.methods")) return []
    return client().call("session.methods", { sessionId }, readSessionMethods).catch(() => [])
  }

  /** Compacts the main line's context. */
  async function compact(sessionId: string) {
    return client().call("session.context.compact", { sessionId, branchId: MAIN_BRANCH }, readCompactResult)
  }

  /** Writes the main line's goal through `session.goal.<call>`; the goal changes when its events arrive. */
  async function writeGoal(sessionId: string, call: GoalCall, objective: string | undefined): Promise<void> {
    await client().call(`session.goal.${call}`, { sessionId, branchId: MAIN_BRANCH, ...(objective === undefined ? {} : { objective }) }, readAck)
  }

  /** Reads the main line's context window usage. */
  async function usage(sessionId: string) {
    return client().call("session.context.usage", { sessionId, branchId: MAIN_BRANCH }, readContextUsage)
  }

  /** Forks the Session at `at` (a main-line cut) and returns the new Session's id. */
  async function fork(sessionId: string, at: number): Promise<string> {
    return client().call("session.fork", { sessionId, at }, readForkedSessionId)
  }

  /** The files an `@` mention offers under the Session's workspace. */
  async function searchFiles(sessionId: string, query: string): Promise<FileCandidate[]> {
    return client().call("session.files.search", { sessionId, query, limit: 20 }, readFileCandidates)
  }

  /** The files a turn's `write` and `edit` changed, with their status now, and the commands it ran. */
  async function turnChanges(sessionId: string, branchId: string, turnId: string): Promise<TurnChanges> {
    return client().call("session.files.changes", { sessionId, branchId, turnId }, readTurnChanges)
  }

  /** Restores `paths` to their content before the turn; the server refuses unless every one is still as the turn left it. */
  async function rewindTurn(sessionId: string, branchId: string, turnId: string, paths: string[]): Promise<RewindResult> {
    return client().call("session.files.rewind", { sessionId, branchId, turnId, paths }, readRewindResult)
  }

  /** Withdraws a queued message; with `steer`, the same content joins the running turn at its next boundary. */
  async function withdrawInput(sessionId: string, inputId: string, steer: UserInput | undefined): Promise<WithdrawResult> {
    return client().call("session.input.withdraw", { sessionId, branchId: MAIN_BRANCH, inputId, ...(steer === undefined ? {} : { steer }) }, readWithdrawResult)
  }

  /** The policy of `main`, or undefined when the Session serves no permission method or the read fails. */
  async function sessionPermission(sessionId: string): Promise<PermissionPolicy | undefined> {
    if (!supports("session.permission.get")) return undefined
    return client().call("session.permission.get", { sessionId, branchId: MAIN_BRANCH }, readPermissionState)
      .then((state) => state.policy, () => undefined)
  }

  /** Changes the preset, preserving tool exceptions; a conflict refreshes without retrying. */
  async function setPermission(sessionId: string, preset: PermissionPreset): Promise<PermissionOutcome> {
    const state = await client().call("session.permission.get", { sessionId, branchId: MAIN_BRANCH }, readPermissionState)
    const view = views[sessionId]
    if (view !== undefined) view.permission = state.policy
    if (state.policy.preset === preset) return "unchanged"
    const policy = { ...state.policy, preset }
    try {
      await client().call("session.permission.update", {
        sessionId,
        branchId: MAIN_BRANCH,
        expectedVersion: state.version,
        policy,
      }, readPermissionVersion)
    } catch (cause) {
      if (cause instanceof RpcError && cause.appError === "permission_conflict") {
        if (view !== undefined) view.permission = await sessionPermission(sessionId)
        return "conflict"
      }
      throw cause
    }
    if (view !== undefined) view.permission = policy
    return "updated"
  }

  async function setEffort(sessionId: string, effort: string | undefined): Promise<string> {
    return client().call("session.model.update", { sessionId, branchId: MAIN_BRANCH, modelPreferences: { reasoningEffort: effort ?? { action: "clear" } } }, readConfigurationOutcome)
  }

  async function setModel(sessionId: string, model: ModelId): Promise<void> {
    const result = await client().call("session.model.update", {
      sessionId,
      branchId: MAIN_BRANCH,
      providerId: model.providerId,
      modelId: model.modelId,
    }, readModelUpdate)
    adoptModel(sessionId, result.model)
  }

  return { submit, stop, createLine, endLine, memberApproval, respond, allowAlways, allowForSession, answerQuestion, setPlanning, reviewPlan, sessionMethods, compact, writeGoal, usage, fork, searchFiles, turnChanges, rewindTurn, withdrawInput, sessionPermission, setPermission, setEffort, setModel, taskOutput, stopTask }
}
