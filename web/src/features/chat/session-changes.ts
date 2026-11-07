import type { TimelineRow } from "../../projection"
import { diffLines, type DiffLine } from "./tool-display"

/** A write's whole content shows up to this many lines. */
const MAX_WRITTEN_LINES = 400

/** One successful `write` or `edit` call's change to a file. */
export type FileEdit = {
  key: string
  lines: DiffLine[]
  added: number
  removed: number
  /** Lines of a written file that were left out of `lines`. */
  hidden: number
}

/** A file the line changed, with its calls in Timeline order. */
export type ChangedFile = { path: string; edits: FileEdit[]; added: number; removed: number }

type ToolRow = Extract<TimelineRow, { kind: "tool" }>

/** Rows are kept for the life of the line, so each call is read once. */
const edits = new WeakMap<ToolRow, FileEdit>()

function changing(row: TimelineRow): row is ToolRow {
  if (row.kind !== "tool" || row.pending || row.result === undefined || row.result.isError) return false
  const name = row.name.trim().toLowerCase()
  return name === "write" || name === "edit"
}

/** The resolved path the call declared writing, else its input path against `cwd`. */
function pathOf(row: ToolRow, cwd: string | undefined): string | undefined {
  const declared = row.result?.filesWritten?.[0]
  if (declared !== undefined) return declared
  const path = typeof row.input.path === "string" ? row.input.path : undefined
  if (path === undefined || path.startsWith("/") || cwd === undefined) return path
  return `${cwd.replace(/\/+$/, "")}/${path.replace(/^\.\//, "")}`
}

function editOf(row: ToolRow): FileEdit {
  const cached = edits.get(row)
  if (cached !== undefined) return cached
  const diff = row.result?.diff
  let lines: DiffLine[]
  let hidden = 0
  if (row.name.trim().toLowerCase() === "edit" || diff !== undefined) lines = diffLines(row.input, diff)
  else {
    const written = typeof row.input.content === "string" ? row.input.content.replace(/\n$/, "").split("\n") : []
    hidden = Math.max(0, written.length - MAX_WRITTEN_LINES)
    lines = written.slice(0, MAX_WRITTEN_LINES).map((text, index) => ({ sign: "+", text, line: index + 1 }))
  }
  const edit = {
    key: row.key,
    lines,
    added: lines.filter((line) => line.sign === "+").length + hidden,
    removed: lines.filter((line) => line.sign === "-").length,
    hidden,
  }
  edits.set(row, edit)
  return edit
}

/**
 * The files a line's successful `write` and `edit` calls changed, sorted by
 * path: an edit, or a write that replaced a file and reported its diff, shows
 * that display diff; any other write shows its content as added lines.
 * Commands' changes are not seen here.
 */
export function sessionChanges(rows: readonly TimelineRow[], cwd: string | undefined): ChangedFile[] {
  const files = new Map<string, ChangedFile>()
  for (const row of rows) {
    if (!changing(row)) continue
    const path = pathOf(row, cwd)
    if (path === undefined) continue
    const edit = editOf(row)
    const file = files.get(path) ?? { path, edits: [], added: 0, removed: 0 }
    file.edits.push(edit)
    file.added += edit.added
    file.removed += edit.removed
    files.set(path, file)
  }
  return [...files.values()].sort((left, right) => left.path.localeCompare(right.path))
}

/** How many files the line's `write` and `edit` calls changed, without reading their diffs. */
export function changedFileCount(rows: readonly TimelineRow[], cwd: string | undefined): number {
  const paths = new Set<string>()
  for (const row of rows) {
    if (!changing(row)) continue
    const path = pathOf(row, cwd)
    if (path !== undefined) paths.add(path)
  }
  return paths.size
}

/** `path` shown under `cwd` when it lies there. */
export function workspacePath(path: string, cwd: string | undefined): string {
  if (cwd === undefined) return path
  const base = `${cwd.replace(/\/+$/, "")}/`
  return path.startsWith(base) ? path.slice(base.length) : path
}
