/** Readers of the `terminal.*` management replies (docs/design/web-workspace.md §4.4). */
import { arr, bool, fail, num, obj, str } from "./read-core"

/** A terminal; `command` is the program it runs in place of a shell (a dev server the preview started). */
export type TerminalSummary = { terminalId: string; cwd: string; title: string; exited: boolean; command?: string[] }

/** Output after a cursor; `reset` says output before it is no longer kept. */
export type TerminalChunk = { data: string; cursor: number; reset: boolean; exited: boolean; exitCode?: number }

export function readOpenedTerminal(value: unknown): TerminalSummary {
  const source = obj(value, "terminal.open")
  return { terminalId: str(source, "terminalId", "terminal.open"), cwd: str(source, "cwd", "terminal.open"), title: str(source, "title", "terminal.open"), exited: false }
}

export function readTerminals(value: unknown): TerminalSummary[] {
  return arr(obj(value, "terminal.list").terminals, "terminal.list.terminals").map((item, index) => {
    const at = `terminal.list.terminals[${index}]`
    const source = obj(item, at)
    const command = source.command === undefined ? undefined : arr(source.command, `${at}.command`).map((part, index) => typeof part === "string" ? part : fail(`${at}.command[${index}]`, "a string"))
    return { terminalId: str(source, "terminalId", at), cwd: str(source, "cwd", at), title: str(source, "title", at), exited: bool(source, "exited", at), ...(command === undefined ? {} : { command }) }
  })
}

export function readTerminalChunk(value: unknown): TerminalChunk {
  const source = obj(value, "terminal.read")
  const chunk: TerminalChunk = { data: str(source, "data", "terminal.read"), cursor: num(source, "cursor", "terminal.read"), reset: bool(source, "reset", "terminal.read"), exited: bool(source, "exited", "terminal.read") }
  if (source.exitCode !== undefined) chunk.exitCode = num(source, "exitCode", "terminal.read")
  return chunk
}
