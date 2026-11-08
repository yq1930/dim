import { ref, watch } from "vue"
import { defineStore } from "pinia"
import { RpcError } from "../rpc/client"
import { readFolderListing, readSavedFile, readTextSearch, type FolderListing, type TextSearch } from "../rpc/files"
import type { FileView } from "../rpc/protocol"
import { useChatStore } from "./chat"
import { useConnectionStore } from "./connection"
import { usePanesStore } from "./panes"

export type ShownFile = Exclude<FileView, { kind: "directory" }>

/** The lines a link named; `seq` is new on every open, so the view scrolls to them again. */
export type FileTarget = { line: number; endLine?: number; seq: number }

/**
 * A file open in a Session's file view. A preview tab gives its place to the next
 * file opened until it is kept. `draft` is the operator's unsaved edit; `stale`
 * says the file changed on disk under it.
 */
export type OpenFile = { path: string; preview: boolean; view?: ShownFile; error?: string; target?: FileTarget; editing?: boolean; draft?: string; stale?: boolean; saving?: boolean }

/** A folder the file tree read, or why it could not. */
export type Folder = { listing?: FolderListing; error?: string }

const WRAP_KEY = "dim-web.files.wrap"
const TREE_KEY = "dim-web.files.tree"

function readFlag(key: string, fallback: boolean): boolean {
  try {
    const value = localStorage.getItem(key)
    return value === null ? fallback : value !== "false"
  } catch {
    return fallback
  }
}

function message(cause: unknown): string {
  return cause instanceof Error && cause.message ? cause.message : String(cause)
}

/**
 * Each Session's open files and file tree, the detail panel's file view. The
 * tabs' paths are remembered with the Session's panes and opened again when its
 * file view is next shown; the rest lasts for this page load.
 */
export const useFilesStore = defineStore("files", () => {
  const chat = useChatStore()
  const panes = usePanesStore()
  const bySession = ref<Record<string, OpenFile[]>>({})
  const active = ref<Record<string, string | undefined>>({})
  const wrap = ref(readFlag(WRAP_KEY, true))
  /** Whether the file tree shows beside the open file. */
  const tree = ref(readFlag(TREE_KEY, true))
  /** Each Session's folders read, by path under its workspace ("" is the workspace). */
  const folders = ref<Record<string, Record<string, Folder>>>({})
  /** Each Session's open folders. */
  const expanded = ref<Record<string, string[]>>({})
  let seq = 0
  const restoring = new Set<string>()

  watch(wrap, (value) => { try { localStorage.setItem(WRAP_KEY, String(value)) } catch { /* kept for this page load */ } })
  watch(tree, (value) => { try { localStorage.setItem(TREE_KEY, String(value)) } catch { /* kept for this page load */ } })

  function supports(sessionId: string, method: string): boolean {
    return chat.views[sessionId]?.methods.includes(method) === true
  }

  function readable(sessionId: string): boolean {
    return supports(sessionId, "session.files.read")
  }

  function update(sessionId: string, path: string, change: (file: OpenFile) => OpenFile): void {
    bySession.value = { ...bySession.value, [sessionId]: (bySession.value[sessionId] ?? []).map((file) => file.path === path ? change(file) : file) }
  }

  function find(sessionId: string, path: string): OpenFile | undefined {
    return bySession.value[sessionId]?.find((file) => file.path === path)
  }

  function remember(sessionId: string): void {
    panes.setFiles(sessionId, (bySession.value[sessionId] ?? []).map((file) => file.path), active.value[sessionId])
  }

  /** Opens the tabs the Session's panes remember, once per page load, unless files were opened meanwhile. */
  async function restore(sessionId: string): Promise<void> {
    const saved = panes.of(sessionId).files
    if (bySession.value[sessionId] !== undefined || restoring.has(sessionId) || saved === undefined || saved.paths.length === 0) return
    restoring.add(sessionId)
    try {
      const read = await Promise.all(saved.paths.map((path) => chat.readFile(sessionId, path).catch(() => undefined)))
      if (bySession.value[sessionId] !== undefined) return
      const opened = read.flatMap((view): OpenFile[] => view === undefined || view.kind === "directory" ? [] : [{ path: view.path, preview: false, view }])
      bySession.value = { ...bySession.value, [sessionId]: opened }
      active.value = { ...active.value, [sessionId]: opened.find((file) => file.path === saved.active)?.path ?? opened.at(-1)?.path }
    } finally {
      restoring.delete(sessionId)
    }
  }

  /**
   * Shows `path` (absolute, `~/…` or relative to the Session's directory) in the
   * file view: in its own tab, else in the preview tab, else in a new one, and
   * opens the panel there. False when the path is a directory or lies outside the
   * Session's workspace, which the view does not show.
   */
  async function open(sessionId: string, path: string, lines?: { line: number; endLine?: number }, keep = false): Promise<boolean> {
    let view: FileView
    try {
      view = await chat.readFile(sessionId, path)
    } catch (cause) {
      if (cause instanceof RpcError && cause.appError === "file_outside_workspace") return false
      throw cause
    }
    if (view.kind === "directory") return false
    const named = lines ?? (view.line === undefined ? undefined : { line: view.line, endLine: view.endLine })
    const target = named === undefined ? undefined : { ...named, seq: ++seq }
    const files = bySession.value[sessionId] ?? []
    const index = files.findIndex((file) => file.path === view.path)
    let next: OpenFile[]
    if (index !== -1) {
      next = files.map((file, at) => at === index ? { ...file, preview: file.preview && !keep, view: file.draft === undefined ? view : file.view, error: undefined, target: target ?? file.target } : file)
    } else {
      const opened: OpenFile = { path: view.path, preview: !keep, view, target }
      // A tab with unsaved changes is never given away.
      const preview = keep ? -1 : files.findIndex((file) => file.preview && file.draft === undefined)
      next = preview === -1 ? [...files, opened] : files.map((file, at) => at === preview ? opened : file)
    }
    bySession.value = { ...bySession.value, [sessionId]: next }
    active.value = { ...active.value, [sessionId]: view.path }
    remember(sessionId)
    panes.show(sessionId, "files")
    return true
  }

  /** Reads an open file again; an unchanged file keeps its view, and one being edited keeps the edit and is marked stale. */
  async function reload(sessionId: string, path: string): Promise<void> {
    try {
      const view = await chat.readFile(sessionId, path)
      update(sessionId, path, (file) => {
        if (view.kind === "directory") return file
        const same = sameContent(file.view, view)
        if (file.error === undefined && same) return file
        if (file.draft !== undefined) return { ...file, error: undefined, stale: !same }
        return { ...file, view, error: undefined, stale: false }
      })
    } catch (cause) {
      update(sessionId, path, (file) => ({ ...file, error: message(cause) }))
    }
  }

  function activate(sessionId: string, path: string | undefined): void {
    active.value = { ...active.value, [sessionId]: path }
    remember(sessionId)
  }

  function keepOpen(sessionId: string, path: string): void {
    update(sessionId, path, (file) => ({ ...file, preview: false }))
  }

  /** Closes a tab, activating its neighbour; its unsaved changes go with it. */
  function close(sessionId: string, path: string): void {
    const files = bySession.value[sessionId] ?? []
    const index = files.findIndex((file) => file.path === path)
    const rest = files.filter((file) => file.path !== path)
    bySession.value = { ...bySession.value, [sessionId]: rest }
    if (active.value[sessionId] === path) activate(sessionId, (rest[index] ?? rest[index - 1])?.path)
    else remember(sessionId)
  }

  /** Starts editing an editable text file; the edit keeps the tab. */
  function edit(sessionId: string, path: string): void {
    update(sessionId, path, (file) => file.view?.kind !== "text" || !file.view.editable ? file : { ...file, preview: false, editing: true })
  }

  /** The operator's text; the same text as the file leaves no draft. */
  function setDraft(sessionId: string, path: string, text: string): void {
    update(sessionId, path, (file) => {
      const saved = file.view?.kind === "text" ? normalize(file.view.text) : undefined
      if (text === saved) {
        const { draft: _, ...rest } = file
        return { ...rest, stale: false }
      }
      return { ...file, draft: text }
    })
  }

  /** Ends editing and drops the unsaved changes, reading the file again. */
  async function discard(sessionId: string, path: string): Promise<void> {
    update(sessionId, path, (file) => {
      const { draft: _, ...rest } = file
      return { ...rest, editing: false, stale: false }
    })
    await reload(sessionId, path)
  }

  /** Ends editing; only a file without unsaved changes. */
  function finish(sessionId: string, path: string): void {
    update(sessionId, path, (file) => file.draft === undefined ? { ...file, editing: false } : file)
  }

  /**
   * Writes the draft over the revision the edit started from, or over whatever
   * is there with `force`. A file changed on disk meanwhile is marked stale and
   * the error is thrown; the draft stays.
   */
  async function save(sessionId: string, path: string, force = false): Promise<void> {
    const file = find(sessionId, path)
    if (file?.draft === undefined || file.view?.kind !== "text" || file.saving) return
    const text = file.draft
    const revision = force ? undefined : file.view.revision
    update(sessionId, path, (current) => ({ ...current, saving: true }))
    try {
      const saved = await useConnectionStore().client.call("session.files.save", { sessionId, path, text, ...(revision === undefined ? {} : { revision }) }, readSavedFile)
      update(sessionId, path, (current) => {
        if (current.view?.kind !== "text") return current
        const { draft: _, ...rest } = current
        const unchanged = current.draft === text
        return {
          ...rest,
          ...(unchanged ? {} : { draft: current.draft }),
          saving: false,
          stale: false,
          view: { ...current.view, text, size: saved.size, revision: saved.revision, ...(saved.modified === undefined ? {} : { modified: saved.modified }) },
        }
      })
    } catch (cause) {
      const changed = cause instanceof RpcError && cause.appError === "file_changed"
      update(sessionId, path, (current) => ({ ...current, saving: false, stale: current.stale === true || changed }))
      throw cause
    }
  }

  /** Reads a folder of the workspace for the file tree. */
  async function readFolder(sessionId: string, folder: string): Promise<void> {
    try {
      const listing = await useConnectionStore().client.call("session.files.list", { sessionId, path: folder }, readFolderListing)
      folders.value = { ...folders.value, [sessionId]: { ...folders.value[sessionId], [folder]: { listing } } }
    } catch (cause) {
      folders.value = { ...folders.value, [sessionId]: { ...folders.value[sessionId], [folder]: { error: message(cause) } } }
    }
  }

  async function toggleFolder(sessionId: string, folder: string): Promise<void> {
    const open = expanded.value[sessionId] ?? []
    if (open.includes(folder)) {
      expanded.value = { ...expanded.value, [sessionId]: open.filter((path) => path !== folder) }
      return
    }
    expanded.value = { ...expanded.value, [sessionId]: [...open, folder] }
    if (folders.value[sessionId]?.[folder] === undefined) await readFolder(sessionId, folder)
  }

  /** Reads the workspace and every open folder again. */
  async function refreshTree(sessionId: string): Promise<void> {
    const open = ["", ...(expanded.value[sessionId] ?? [])]
    await Promise.all(open.map((folder) => readFolder(sessionId, folder)))
  }

  function collapseTree(sessionId: string): void {
    expanded.value = { ...expanded.value, [sessionId]: [] }
  }

  function grep(sessionId: string, query: string): Promise<TextSearch> {
    return useConnectionStore().client.call("session.files.grep", { sessionId, query }, readTextSearch, 30_000)
  }

  return { bySession, active, wrap, tree, folders, expanded, supports, readable, open, restore, reload, activate, keepOpen, close, edit, setDraft, discard, finish, save, readFolder, toggleFolder, refreshTree, collapseTree, grep }
})

/** Whether a file read again has the content shown: by revision where both have one, else by size and time. */
function sameContent(shown: ShownFile | undefined, read: ShownFile): boolean {
  if (shown === undefined) return false
  if (shown.kind === "text" && read.kind === "text" && shown.revision !== undefined && read.revision !== undefined) return shown.revision === read.revision
  return shown.modified === read.modified && shown.size === read.size
}

/** A file's text as a text field holds it: LF line ends. */
export function normalize(text: string): string {
  return text.replace(/\r\n?/g, "\n")
}
