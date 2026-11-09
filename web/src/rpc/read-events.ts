/** Readers for Session events and notifications, the facts the timeline folds. */
import type { ApprovalRequest, AskCall, AskQuestion, PlanCall, MemberApprovalDetail, BranchSource, EventContent, Notification, RawBlock, SessionEvent, ToolResult, UserInput, WithdrawResult } from "./protocol"
import { fail, obj, arr, str, optStr, num, bool, modelId, readPermissionPolicy, type Json } from "./read-core"

/** A user message's content blocks, kept as the server wrote them so a steer can resend them unchanged. */
function readUserInput(user: Json, at: string): UserInput {
  return { content: arr(user.content, `${at}.content`).map((item, index) => obj(item, `${at}.content[${index}]`)) }
}

/** `session.input.withdraw`'s status; a steer adds a replacement input that arrives as its own event. */
export function readWithdrawResult(value: unknown): WithdrawResult {
  const at = "session.input.withdraw"
  const status = str(obj(value, at), "status", at)
  if (status === "deleted" || status === "notHeld") return { status }
  fail(`${at}.status`, "deleted or notHeld")
}

const SKILL_BLOCK = '<dim-skill name="'

/** A loaded Skill's block names it after the marker (Skills §9). */
function loadedSkill(text: string): string | undefined {
  if (!text.startsWith(SKILL_BLOCK)) return undefined
  const end = text.indexOf('"', SKILL_BLOCK.length)
  if (end < 0) return undefined
  return text.slice(SKILL_BLOCK.length, end)
    .replaceAll("&lt;", "<")
    .replaceAll("&gt;", ">")
    .replaceAll("&quot;", '"')
    .replaceAll("&amp;", "&")
}

/**
 * The operator's own text of a message and the Skills its blocks loaded: a
 * text block after the first that starts with the marker is a loaded Skill,
 * shown as one line and never as its instructions.
 */
function userText(blocks: unknown, at: string): { text: string; skills: string[] } {
  const text: string[] = []
  const skills: string[] = []
  arr(blocks, at).forEach((item, index) => {
    const block = obj(item, `${at}[${index}]`)
    if (block.type !== "text" || typeof block.text !== "string") return
    const skill = index === 0 ? undefined : loadedSkill(block.text)
    if (skill === undefined) text.push(block.text)
    else skills.push(skill)
  })
  return { text: text.join("\n"), skills }
}

function textOf(blocks: unknown, at: string): string {
  return arr(blocks, at).flatMap((item, index) => {
    const block = obj(item, `${at}[${index}]`)
    return block.type === "text" ? [str(block, "text", `${at}[${index}]`)] : []
  }).join("\n")
}

function toolResult(value: unknown, at: string): ToolResult {
  const source = obj(value, at)
  const text = typeof source.content === "string" ? source.content : textOf(source.content, `${at}.content`)
  const structured = source.structuredContent === undefined || source.structuredContent === null ? undefined : obj(source.structuredContent, `${at}.structuredContent`)
  const diff = typeof structured?.diff === "string" ? structured.diff : undefined
  const approved = typeof structured?.approved === "boolean" ? structured.approved : undefined
  const effects = source.effects === undefined || source.effects === null ? [] : arr(source.effects, `${at}.effects`)
  const filesWritten = effects.flatMap((item, index) => {
    const effect = obj(item, `${at}.effects[${index}]`)
    return effect.kind === "fileWritten" ? [str(effect, "path", `${at}.effects[${index}]`)] : []
  })
  return { text, isError: source.isError === true, ...(diff === undefined ? {} : { diff }), ...(approved === undefined ? {} : { approved }), ...(filesWritten.length === 0 ? {} : { filesWritten }) }
}

function rawBlocks(blocks: unknown, at: string): RawBlock[] {
  if (blocks === undefined || blocks === null) return []
  return arr(blocks, at).flatMap((item, index) => {
    const block = obj(item, `${at}[${index}]`)
    if (block.type !== "raw") return []
    const key = optStr(block, "key", `${at}[${index}]`)
    return [{ text: str(block, "text", `${at}[${index}]`), ...(key === undefined ? {} : { key }), retire: block.retire === true }]
  })
}

function branchSource(value: unknown, at: string): BranchSource {
  const source = obj(value, at)
  const kind = str(source, "kind", at)
  if (kind === "tool") {
    return {
      kind,
      turnId: str(source, "turn_id", at),
      requestId: str(source, "request_id", at),
      callId: str(source, "call_id", at),
    }
  }
  if (kind === "host") return { kind, operationId: str(source, "operation_id", at) }
  return fail(at, "a branch source")
}

function branchCut(value: unknown, at: string): number | undefined {
  const source = obj(value, at)
  switch (str(source, "kind", at)) {
    case "sequence": return num(source, "sequence", at)
    case "create": return undefined
    default: return fail(at, "a branch cut")
  }
}

function content(type: string, name: string, data: Json, at: string): EventContent {
  switch (`${type}/${name}`) {
    case "exec/started":
      return { kind: "exec-started", taskId: str(data, "task_id", at), callId: str(data, "call_id", at), description: str(data, "description", at) }
    case "exec/ended": {
      const status = str(data, "status", at)
      if (status !== "completed" && status !== "failed" && status !== "cancelled") fail(`${at}.status`, "an exec outcome")
      return { kind: "exec-ended", taskId: str(data, "task_id", at), status, result: toolResult(data.result, `${at}.result`) }
    }
    case "input/message": {
      const blocks = rawBlocks(data.blocks, `${at}.blocks`)
      if (data.user === undefined || data.user === null) {
        if (blocks.length === 0) return { kind: "input", blocks }
        const mode = optStr(data, "mode", at)
        const text = blocks.map((block) => block.text).find((value) => value.startsWith("<dim-branch-")) ?? blocks[0]!.text
        return { kind: "source-input", text, blocks, ...(mode === undefined ? {} : { mode }) }
      }
      const user = obj(data.user, `${at}.user`)
      const { text, skills } = userText(user.content, `${at}.user.content`)
      return { kind: "input", text, user: readUserInput(user, `${at}.user`), skills: skills.length === 0 ? undefined : skills, blocks }
    }
    case "input/applied":
      return { kind: "input-applied", inputId: str(data, "input_id", at) }
    case "permission/changed":
      return { kind: "permission-changed", policy: readPermissionPolicy(data.policy, `${at}.policy`) }
    case "input/deleted":
      return { kind: "input-deleted", inputId: str(data, "input_id", at) }
    case "branch/start":
      return { kind: "branch-start" }
    case "session/forked":
      return { kind: "session-forked" }
    case "branch/create":
      return {
        kind: "branch-create",
        branchId: str(data, "branch", at),
        at: branchCut(data.at, `${at}.at`),
        prompt: str(data, "prompt", at),
        source: branchSource(data.source, `${at}.source`),
      }
    case "branch/create-failed": {
      const status = str(data, "status", at)
      if (status !== "failed" && status !== "cancelled") fail(`${at}.status`, "a creation outcome")
      return {
        kind: "branch-create-failed",
        branchId: str(data, "branch", at),
        status,
        error: str(data, status === "failed" ? "error" : "reason", at),
      }
    }
    case "branch/end":
      return {
        kind: "branch-end",
        source: branchSource(data.source, `${at}.source`),
        ...(data.ancestor === undefined || data.ancestor === null ? {} : { ancestor: str(data, "ancestor", at) }),
      }
    case "branch/ended":
      return {
        kind: "branch-ended",
        branchId: str(data, "branch", at),
        source: branchSource(data.source, `${at}.source`),
        ...(data.ancestor === undefined || data.ancestor === null ? {} : { ancestor: str(data, "ancestor", at) }),
      }
    case "turn/start":
      return { kind: "turn-start", turnId: str(data, "turn_id", at) }
    case "turn/end": {
      const status = str(data, "status", at)
      if (status !== "completed" && status !== "failed" && status !== "cancelled") fail(`${at}.status`, "a turn outcome")
      const error = status === "failed" ? str(data, "error", at) : undefined
      return { kind: "turn-end", turnId: str(data, "turn_id", at), outcome: status, ...(error === undefined ? {} : { error }) }
    }
    case "step/model-start":
      return { kind: "model-start", requestId: str(data, "request_id", at) }
    case "step/thinking":
      return { kind: "thinking", requestId: str(data, "request_id", at), delta: str(data, "delta", at) }
    case "step/text":
      return { kind: "text", requestId: str(data, "request_id", at), delta: str(data, "delta", at) }
    case "step/tool-call":
      return {
        kind: "tool-call",
        requestId: str(data, "request_id", at),
        asks: readAsks(arr(data.calls, `${at}.calls`), `${at}.calls`),
        plans: readPlans(arr(data.calls, `${at}.calls`), `${at}.calls`),
      }
    case "step/tool-use": {
      const call = obj(data.call, `${at}.call`)
      return {
        kind: "tool-use",
        requestId: str(data, "request_id", at),
        callId: str(call, "id", `${at}.call`),
        name: str(call, "name", `${at}.call`),
        input: obj(call.input, `${at}.call.input`),
      }
    }
    case "step/tool-completed":
      return { kind: "tool-completed", callId: str(data, "call_id", at), result: toolResult(data.result, `${at}.result`) }
    case "step/tool-result":
      return {
        kind: "tool-result",
        requestId: str(data, "request_id", at),
        results: arr(data.results, `${at}.results`).map((item, index) => {
          const result = obj(item, `${at}.results[${index}]`)
          return { callId: str(result, "call_id", `${at}.results[${index}]`), result: toolResult(result.result, `${at}.results[${index}].result`) }
        }),
      }
    case "step/model-output-end": {
      const error = str(data, "status", at) === "failed" ? str(data, "error", at) : undefined
      return { kind: "model-end", requestId: str(data, "request_id", at), ...(error === undefined ? {} : { error }) }
    }
    case "context/configured": {
      const preferences = data.model_preferences === undefined ? undefined : obj(data.model_preferences, `${at}.model_preferences`)
      const reasoningEffort = preferences === undefined ? undefined : optStr(preferences, "reasoningEffort", `${at}.model_preferences`)
      return { kind: "context-configured", model: { providerId: str(data, "provider_id", at), modelId: str(data, "model_id", at) }, ...(reasoningEffort === undefined ? {} : { reasoningEffort }) }
    }
    default:
      return { kind: "other" }
  }
}

/**
 * The `ask_user` input as Questions A.1 checks it. `undefined` when the input
 * breaks a rule: the server rejects such a call at once, so it gets no prompt.
 */
function readAskQuestions(input: Json): AskQuestion[] | undefined {
  if (!Array.isArray(input.questions) || input.questions.length < 1 || input.questions.length > 4) return undefined
  const ids = new Set<string>()
  const questions: AskQuestion[] = []
  for (const [index, item] of input.questions.entries()) {
    const question = askQuestion(item, String(index + 1))
    if (question === undefined || ids.has(question.id)) return undefined
    ids.add(question.id)
    questions.push(question)
  }
  return questions
}

function askQuestion(value: unknown, defaultId: string): AskQuestion | undefined {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return undefined
  const source = value as Json
  const id = source.id ?? defaultId
  if (typeof id !== "string" || id === "") return undefined
  if (source.header !== undefined && typeof source.header !== "string") return undefined
  if (typeof source.header === "string" && [...source.header].length > 40) return undefined
  if (typeof source.question !== "string" || source.question.trim() === "") return undefined
  if (!Array.isArray(source.options) || source.options.length < 2 || source.options.length > 6) return undefined
  const labels = new Set<string>()
  const options: AskQuestion["options"] = []
  for (const item of source.options) {
    if (item === null || typeof item !== "object" || Array.isArray(item)) return undefined
    const option = item as Json
    if (typeof option.label !== "string" || option.label.trim() === "" || labels.has(option.label)) return undefined
    if (option.description !== undefined && typeof option.description !== "string") return undefined
    labels.add(option.label)
    options.push({ label: option.label, ...(typeof option.description === "string" ? { description: option.description } : {}) })
  }
  if ((source.multiSelect !== undefined && typeof source.multiSelect !== "boolean") || (source.allowCustom !== undefined && typeof source.allowCustom !== "boolean")) return undefined
  return {
    id,
    ...(typeof source.header === "string" ? { header: source.header } : {}),
    question: source.question,
    options,
    multiSelect: source.multiSelect === true,
    allowCustom: source.allowCustom !== false,
  }
}

function readAsks(calls: unknown[], at: string): AskCall[] {
  const asks: AskCall[] = []
  for (const [index, item] of calls.entries()) {
    const call = obj(item, `${at}[${index}]`)
    if (str(call, "name", `${at}[${index}]`) !== "ask_user") continue
    const questions = readAskQuestions(obj(call.input, `${at}[${index}].input`))
    if (questions !== undefined) asks.push({ callId: str(call, "id", `${at}[${index}]`), questions })
  }
  return asks
}

function readPlans(calls: unknown[], at: string): PlanCall[] {
  const plans: PlanCall[] = []
  for (const [index, item] of calls.entries()) {
    const call = obj(item, `${at}[${index}]`)
    if (str(call, "name", `${at}[${index}]`) !== "exit_plan_mode") continue
    const plan = obj(call.input, `${at}[${index}].input`).plan
    if (typeof plan === "string" && plan.trim() !== "") plans.push({ callId: str(call, "id", `${at}[${index}]`), plan })
  }
  return plans
}

export function readEvent(value: unknown, at: string): SessionEvent {
  const source = obj(value, at)
  const type = str(source, "type", at)
  const name = str(source, "name", at)
  return {
    id: str(source, "id", at),
    sequence: num(source, "sequence", at),
    createAt: num(source, "create_at", at),
    content: content(type, name, obj(source.data, `${at}.data`), `${at}.data`),
  }
}


function readApproval(params: Json): ApprovalRequest {
  const at = "approval.request"
  const call = obj(params.call, `${at}.call`)
  const source = obj(params.source, `${at}.source`)
  const callId = source.kind === "tool" ? str(source, "call_id", `${at}.source`) : undefined
  return {
    sessionId: str(params, "sessionId", at),
    branchId: str(params, "branchId", at),
    requestId: str(params, "requestId", at),
    ...(callId === undefined ? {} : { callId }),
    call: { name: str(call, "name", `${at}.call`), input: obj(call.input, `${at}.call.input`) },
    reason: str(params, "reason", at),
  }
}

export function readMemberApproval(value: unknown): MemberApprovalDetail {
  const at = "session.collaboration.approval.get"
  const source = obj(value, at)
  if (source.state === "ended") return { state: "ended" }
  if (source.state !== "pending") fail(at, "pending or ended")
  const descriptor = obj(source.descriptor, `${at}.descriptor`)
  const member = obj(descriptor.member, `${at}.member`)
  const call = obj(descriptor.call, `${at}.call`)
  return { state: "pending", approvalId: str(source, "approvalId", at), descriptor: {
    delegationId: str(descriptor, "delegationId", at), requestId: str(descriptor, "requestId", at), reason: str(descriptor, "reason", at),
    member: { agentId: str(member, "agentId", at), sessionId: str(member, "sessionId", at), branchId: str(member, "branchId", at) },
    call: { name: str(call, "name", at), input: obj(call.input, at) },
  } }
}

/** Decodes a server notification; methods the web workspace does not use read as undefined. */
export function readNotification(method: string, value: unknown): Notification | undefined {
  const params = obj(value ?? {}, method)
  switch (method) {
    case "session.event":
      return {
        method,
        sessionId: str(params, "sessionId", method),
        branchId: str(params, "branchId", method),
        event: readEvent(params.event, `${method}.event`),
      }
    case "session.state":
      return {
        method,
        sessionId: str(params, "sessionId", method),
        branchId: str(params, "branchId", method),
        running: bool(params, "running", method),
        stopping: bool(params, "stopping", method),
      }
    case "session.model.progress": {
      const requestId = optStr(params, "requestId", method)
      return {
        method,
        progress: {
          sessionId: str(params, "sessionId", method),
          branchId: str(params, "branchId", method),
          ...(requestId === undefined ? {} : { requestId }),
          text: str(params, "text", method),
          textOffset: num(params, "textOffset", method),
          thinking: str(params, "thinking", method),
          thinkingOffset: num(params, "thinkingOffset", method),
        },
      }
    }
    case "approval.request":
      return { method, request: readApproval(params) }
    case "approval.resolved":
    case "approval.cancelled":
      return {
        method,
        sessionId: str(params, "sessionId", method),
        branchId: str(params, "branchId", method),
        requestId: str(params, "requestId", method),
      }
    case "session.notice": {
      if (params.source === "SessionStore" && params.kind === "title") {
        const data = obj(params.data, method)
        return { method: "sessionTitleChanged", sessionId: str(params, "sessionId", method), title: optStr(data, "title", method), titleRevision: num(data, "titleRevision", method) }
      }
      if (params.source !== "Collaboration" || params.kind !== "memberApprovalEnded") return undefined
      const data = obj(params.data, method)
      return { method: "memberApprovalEnded", sessionId: str(params, "sessionId", method), branchId: str(data, "branchId", method), approvalId: str(data, "approvalId", method) }
    }
    case "session.closed": {
      const reason = optStr(params, "reason", method)
      return { method, sessionId: str(params, "sessionId", method), ...(reason === undefined ? {} : { reason }) }
    }
    default:
      return undefined
  }
}
