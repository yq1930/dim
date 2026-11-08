import { fileLink } from "../../mentions"

/**
 * What adding selected lines of a file to the message inserts: a link to the
 * lines, then their text in a fence longer than any backtick run inside it.
 */
export function selectionQuote(selection: { path: string; label: string; line: number; endLine: number; text: string; lang?: string }): string {
  const lines = selection.line === selection.endLine ? `${selection.line}` : `${selection.line}-${selection.endLine}`
  const body = selection.text.replace(/\n$/, "")
  const longest = Math.max(2, ...[...body.matchAll(/`+/g)].map((run) => run[0].length))
  const fence = "`".repeat(longest + 1)
  return `${fileLink(`${selection.label}:${lines}`, `${selection.path}:${lines}`)}\n${fence}${selection.lang ?? ""}\n${body}\n${fence}`
}
