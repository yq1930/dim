/**
 * Strict readers for App Server replies and notifications: the one place the
 * web workspace parses wire data. A value that does not match its contract
 * throws a ProtocolError here; nothing past these files re-checks shapes. This
 * module reads Session replies and re-exports the Provider and event readers.
 */
import type { CompactResult, ContextUsage, FileCandidate, FileView, PermissionState, BranchCreateResult, CreateResult, HelloResult, HistoryPage, ModelUpdateResult, SessionSummary, TurnChanges, RewindResult } from "./protocol"
import { fail, obj, arr, str, optStr, num, finite, bool, modelId, readPermissionPolicy } from "./read-core"
import { readEvent } from "./read-events"

export function readHello(value: unknown): HelloResult {
  const source = obj(value, "hello")
  return {
    instanceId: str(source, "instanceId", "hello"),
    serverVersion: str(source, "serverVersion", "hello"),
    protocolVersion: num(source, "protocolVersion", "hello"),
    features: arr(source.features, "hello.features").map((feature, index) => {
      if (typeof feature !== "string") fail(`hello.features[${index}]`, "a string")
      return feature
    }),
  }
}

/** A reply whose content the caller does not use (`{}`, a barrier, the submitted event). */
export function readAck(value: unknown): void {
  obj(value, "reply")
}

export function readExecOutput(value: unknown): string {
  return str(obj(value, "session.exec.output"), "text", "session.exec.output")
}

/** `session.methods` of one Session: the method names it serves. */
export function readSessionMethods(value: unknown): string[] {
  const at = "session.methods"
  return arr(obj(value, at).methods, `${at}.methods`).map((item, index) => str(obj(item, `${at}.methods[${index}]`), "name", `${at}.methods[${index}]`))
}

export function readCompactResult(value: unknown): CompactResult {
  const at = "session.context.compact"
  const source = obj(value, at)
  const status = str(source, "status", at)
  if (status === "completed") return { status }
  if (status === "skipped") return { status, reason: str(source, "reason", at) }
  fail(`${at}.status`, "completed or skipped")
}

export function readContextUsage(value: unknown): ContextUsage {
  const at = "session.context.usage"
  const source = obj(value, at)
  return {
    contextWindow: finite(source, "contextWindow", at),
    outputTokens: finite(source, "outputTokens", at),
    thresholdTokens: finite(source, "thresholdTokens", at),
    estimatedInputTokens: finite(source, "estimatedInputTokens", at),
    baselineSource: str(source, "baselineSource", at),
  }
}

/** `session.fork`: the new Session's id. */
export function readForkedSessionId(value: unknown): string {
  return str(obj(value, "session.fork"), "sessionId", "session.fork")
}

export function readSummaries(value: unknown): SessionSummary[] {
  return arr(value, "session.summaries").map((item, index) => {
    const at = `session.summaries[${index}]`
    const source = obj(item, at)
    const preview = optStr(source, "preview", at)
    const title = optStr(source, "title", at)
    return {
      sessionId: str(source, "sessionId", at),
      cwd: str(source, "cwd", at),
      model: modelId(source.model, `${at}.model`),
      createdAt: num(source, "createdAt", at),
      running: bool(source, "running", at),
      sequence: num(source, "sequence", at),
      ...(preview === undefined ? {} : { preview }),
      ...(title === undefined ? {} : { title }),
      ...(source.titleRevision === undefined ? {} : { titleRevision: num(source, "titleRevision", at) }),
      updatedAt: num(source, "updatedAt", at),
      ...(source.archived === undefined ? {} : { archived: bool(source, "archived", at) }),
    }
  })
}

export function readCreate(value: unknown): CreateResult {
  const source = obj(value, "session.create")
  return {
    sessionId: str(source, "sessionId", "session.create"),
    model: modelId(source.model, "session.create.model"),
    sequence: num(source, "sequence", "session.create"),
  }
}

export function readRenamedTitle(value: unknown): { title: string | undefined; titleRevision: number } {
  const source = obj(value, "session.rename")
  return { title: optStr(source, "title", "session.rename"), titleRevision: num(source, "revision", "session.rename") }
}

export function readBranchCreate(value: unknown): BranchCreateResult {
  const source = obj(value, "session.branch_create")
  const branch = obj(source.branch, "session.branch_create.branch")
  const at = "session.branch_create.branch"
  return {
    branchId: str(branch, "branchId", at),
    ...(branch.parentBranchId === undefined || branch.parentBranchId === null ? {} : { parentBranchId: str(branch, "parentBranchId", at) }),
    ...(branch.branchPoint === undefined || branch.branchPoint === null ? {} : { branchPoint: num(branch, "branchPoint", at) }),
    ...(branch.createEvent === undefined || branch.createEvent === null ? {} : { createEvent: num(branch, "createEvent", at) }),
  }
}

export function readHistory(value: unknown): HistoryPage {
  const source = obj(value, "session.history")
  return {
    events: arr(source.events, "session.history.events").map((event, index) => readEvent(event, `session.history.events[${index}]`)),
    through: num(source, "through", "session.history"),
    nextAfter: num(source, "nextAfter", "session.history"),
    done: bool(source, "done", "session.history"),
  }
}

export function readModelUpdate(value: unknown): ModelUpdateResult {
  const source = obj(value, "session.model.update")
  return { model: modelId(source.model, "session.model.update.model"), outcome: str(source, "outcome", "session.model.update") }
}

export function readPermissionState(value: unknown): PermissionState {
  const at = "session.permission.get"
  const source = obj(value, at)
  return { version: num(source, "version", at), policy: readPermissionPolicy(source.policy, `${at}.policy`) }
}

/** `session.permission.update`: the version the new policy is stored under. */
export function readPermissionVersion(value: unknown): number {
  return num(obj(value, "session.permission.update"), "version", "session.permission.update")
}

/** `session.files.search`: the files an `@` mention offers, best first. */
export function readFileCandidates(value: unknown): FileCandidate[] {
  const at = "session.files.search"
  return arr(obj(value, at).candidates, `${at}.candidates`).map((item, index) => {
    const entry = `${at}.candidates[${index}]`
    const source = obj(item, entry)
    return { relativePath: str(source, "relativePath", entry), absolutePath: str(source, "absolutePath", entry), dir: bool(source, "dir", entry) }
  })
}

/** `session.files.read`: the file, or that the path is a directory. */
export function readFileView(value: unknown): FileView {
  const at = "session.files.read"
  const source = obj(value, at)
  const place = {
    path: str(source, "path", at),
    ...(source.relativePath === undefined ? {} : { relativePath: str(source, "relativePath", at) }),
    ...(source.line === undefined ? {} : { line: num(source, "line", at) }),
    ...(source.endLine === undefined ? {} : { endLine: num(source, "endLine", at) }),
  }
  if (source.kind === "directory") return { ...place, kind: "directory" }
  const file = { size: num(source, "size", at), ...(source.modified === undefined ? {} : { modified: num(source, "modified", at) }) }
  switch (source.kind) {
    case "binary": return { ...place, ...file, kind: "binary" }
    case "image": return { ...place, ...file, kind: "image", mime: str(source, "mime", at), data: str(source, "data", at) }
    case "text": return { ...place, ...file, kind: "text", text: str(source, "text", at), truncated: bool(source, "truncated", at), lossy: bool(source, "lossy", at) }
    default: return fail(`${at}.kind`, "directory, binary, image or text")
  }
}

/** `session.files.changes`: a turn's file changes, each with its status now, and its commands. */
export function readTurnChanges(value: unknown): TurnChanges {
  const at = "session.files.changes"
  const source = obj(value, at)
  return {
    ended: bool(source, "ended", at),
    files: arr(source.files, `${at}.files`).map((item, index) => {
      const entry = `${at}.files[${index}]`
      const file = obj(item, entry)
      const change = str(file, "change", entry)
      if (change !== "modified" && change !== "created") fail(`${entry}.change`, "a change kind")
      const status = str(file, "status", entry)
      if (status !== "restorable" && status !== "restored" && status !== "changed" && status !== "unrecorded") fail(`${entry}.status`, "a file status")
      return { path: str(file, "path", entry), change, calls: num(file, "calls", entry), status }
    }),
    commands: arr(source.commands, `${at}.commands`).map((item, index) => {
      const entry = `${at}.commands[${index}]`
      const command = obj(item, entry)
      const exitCode = command.exitCode
      if (exitCode !== undefined && (typeof exitCode !== "number" || !Number.isSafeInteger(exitCode))) fail(`${entry}.exitCode`, "an integer")
      const signal = optStr(command, "signal", entry)
      return { command: str(command, "command", entry), ...(exitCode === undefined ? {} : { exitCode }), ...(signal === undefined ? {} : { signal }) }
    }),
  }
}

/** `session.files.rewind`: what the undo restored and removed, and the file it stopped at. */
export function readRewindResult(value: unknown): RewindResult {
  const at = "session.files.rewind"
  const source = obj(value, at)
  const paths = (key: string): string[] => arr(source[key], `${at}.${key}`).map((item, index) => {
    if (typeof item !== "string") fail(`${at}.${key}[${index}]`, "a string")
    return item
  })
  const failed = source.failed === undefined ? undefined : obj(source.failed, `${at}.failed`)
  return {
    restored: paths("restored"),
    removed: paths("removed"),
    ...(failed === undefined ? {} : { failed: { path: str(failed, "path", `${at}.failed`), message: str(failed, "message", `${at}.failed`) } }),
    recorded: bool(source, "recorded", at),
  }
}

/** `attachment.put`: the stored text's path, which the message names. */
export function readAttachmentPath(value: unknown): string {
  return str(obj(value, "attachment.put"), "path", "attachment.put")
}

/** The `outcome` of a `session.model.update` that only changes preferences. */
export function readConfigurationOutcome(value: unknown): string {
  return str(obj(value, "session.model.update"), "outcome", "session.model.update")
}

export { ProtocolError } from "./read-core"
export * from "./read-providers"
export * from "./read-events"
