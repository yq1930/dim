/** Readers of the file pane's `session.files.*` replies beyond `read` (WorkspaceTools, File view). */
import { arr, bool, num, obj, str } from "./read-core"

/** One folder of the workspace as the file tree lists it, folders first. */
export type FolderListing = { path: string; relativePath: string; entries: { name: string; dir: boolean }[]; truncated: boolean }

/** A line that contains the searched text; `path` is relative to the workspace. */
export type TextMatch = { path: string; line: number; column: number; text: string }
export type TextSearch = { matches: TextMatch[]; truncated: boolean }

/** A saved file's new state. */
export type SavedFile = { size: number; modified?: number; revision: string }

export function readFolderListing(value: unknown): FolderListing {
  const at = "session.files.list"
  const source = obj(value, at)
  return {
    path: str(source, "path", at),
    relativePath: str(source, "relativePath", at),
    entries: arr(source.entries, `${at}.entries`).map((item, index) => {
      const entry = obj(item, `${at}.entries[${index}]`)
      return { name: str(entry, "name", `${at}.entries[${index}]`), dir: bool(entry, "dir", `${at}.entries[${index}]`) }
    }),
    truncated: bool(source, "truncated", at),
  }
}

export function readTextSearch(value: unknown): TextSearch {
  const at = "session.files.grep"
  const source = obj(value, at)
  return {
    matches: arr(source.matches, `${at}.matches`).map((item, index) => {
      const entry = `${at}.matches[${index}]`
      const found = obj(item, entry)
      return { path: str(found, "path", entry), line: num(found, "line", entry), column: num(found, "column", entry), text: str(found, "text", entry) }
    }),
    truncated: bool(source, "truncated", at),
  }
}

export function readSavedFile(value: unknown): SavedFile {
  const at = "session.files.save"
  const source = obj(value, at)
  const modified = source.modified === undefined ? undefined : num(source, "modified", at)
  return { size: num(source, "size", at), revision: str(source, "revision", at), ...(modified === undefined ? {} : { modified }) }
}
