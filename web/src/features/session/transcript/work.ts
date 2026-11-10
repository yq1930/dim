/**
 * The chat page's grouping of timeline rows: consecutive reasoning and tool
 * rows read as one work group ("Thought for 12s · 3 reads, 1 command") that
 * expands into its items; messages stand on their own. The thinking view
 * takes each thought out of its group, so only tool calls fold; the verbose
 * view gives every item a row of its own.
 */
import type { TimelineRow } from "../../../projection"
import type { ToolResult } from "../../../rpc/protocol"
import type { TranscriptView } from "../../../stores/preferences"
import { resolveToolDisplay } from "./tool-display"

/** What a work group's summary counts a tool call as. */
export type ToolCategory = "read" | "edit" | "command" | "search" | "web_search" | "web_fetch" | "delegate" | "unknown"

export type WorkItem =
  /** `startedAt` is when the request began; `durationMs` is how long the written thought took from it. */
  | { id: string; kind: "thinking"; content: string; running: boolean; startedAt?: number; durationMs?: number }
  | {
    id: string
    kind: "tool"
    name: string
    category: ToolCategory
    /** The collapsed row's inline summary: command, pattern, URL or file name. */
    primary?: string
    status: "running" | "complete" | "error"
    input: Record<string, unknown>
    result?: ToolResult
  }

export type DisplayRow =
  | { key: string; kind: "work"; items: WorkItem[] }
  | { key: string; kind: "row"; row: Exclude<TimelineRow, { kind: "thinking" | "tool" }> }

export function groupRows(rows: readonly TimelineRow[], view: TranscriptView = "normal"): DisplayRow[] {
  const display: DisplayRow[] = []
  let work: WorkItem[] = []
  const flush = (): void => {
    if (work.length > 0) display.push({ key: `work-${work[0]!.id}`, kind: "work", items: work })
    work = []
  }
  for (const row of rows) {
    if (view === "verbose") flush()
    if (row.kind === "thinking") {
      if (row.text === "" && !row.streaming) continue
      if (view === "thinking") flush()
      work.push({
        id: row.key,
        kind: "thinking",
        content: row.text,
        running: row.streaming,
        ...(row.startedAt === undefined ? {} : { startedAt: row.startedAt }),
        ...(row.elapsedMs === undefined ? {} : { durationMs: row.elapsedMs }),
      })
      if (view === "thinking") flush()
    } else if (row.kind === "tool") {
      const category = toolCategory(row.name, row.input)
      const primary = toolPrimary(category, row.input)
      work.push({
        id: row.key,
        kind: "tool",
        name: row.name,
        category,
        ...(primary === undefined ? {} : { primary }),
        status: row.pending ? "running" : row.result?.isError === true ? "error" : "complete",
        input: row.input,
        ...(row.result === undefined ? {} : { result: row.result }),
      })
    } else {
      flush()
      display.push({ key: row.key, kind: "row", row })
    }
  }
  flush()
  return display
}

const PREVIEW_CHARS = 160

/** The end of a thought under way, as one line: what a folded row shows of it. */
export function thinkingPreview(content: string): string {
  const text = content.trimEnd()
  const line = text.slice(text.lastIndexOf("\n") + 1).replace(/^[\s#>-]+/, "").replace(/[*`]+/g, "")
  return line.length <= PREVIEW_CHARS ? line : line.slice(-PREVIEW_CHARS)
}

/**
 * How long a group's thoughts took: the written ones, and the one under way up to `now`.
 * `timed` is false where no thought carries a time.
 */
export function thinkingTime(items: readonly WorkItem[], now: number): { count: number; running: boolean; timed: boolean; ms: number } {
  let count = 0
  let running = false
  let timed = false
  let ms = 0
  for (const item of items) {
    if (item.kind !== "thinking") continue
    count++
    const elapsed = item.running ? (item.startedAt === undefined ? undefined : Math.max(0, now - item.startedAt)) : item.durationMs
    running ||= item.running
    if (elapsed === undefined) continue
    timed = true
    ms += elapsed
  }
  return { count, running, timed, ms }
}

function toolCategory(name: string, input: Record<string, unknown>): ToolCategory {
  if (name === "agent") return "delegate"
  const { renderer, labelKey } = resolveToolDisplay(name, input)
  switch (renderer) {
    case "command": return "command"
    case "search": return "search"
    case "file": return labelKey === "tool.read" ? "read" : "edit"
    case "web": return labelKey === "tool.webSearch" ? "web_search" : "web_fetch"
    case "generic": return "unknown"
  }
}

function toolPrimary(category: ToolCategory, input: Record<string, unknown>): string | undefined {
  const keys = category === "command" ? ["command"]
    : category === "web_search" ? ["query"]
      : category === "web_fetch" ? ["url"]
        : category === "search" ? ["pattern", "path"]
          : category === "delegate" ? ["prompt", "content", "action"]
            : ["path"]
  const value = keys.map((key) => input[key]).find((candidate): candidate is string => typeof candidate === "string" && candidate.trim() !== "")
  if (value === undefined) return undefined
  const shown = category === "read" || category === "edit" ? value.replaceAll("\\", "/").split("/").at(-1) ?? value
    : category === "delegate" ? value.split("\n")[0]!.trim()
      : value
  return shown.length <= 80 ? shown : `${shown.slice(0, 79)}…`
}
