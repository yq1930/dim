import { ref, watch } from "vue"
import { defineStore } from "pinia"
import { RpcError } from "../rpc/client"
import type { FileView } from "../rpc/protocol"
import { useChatStore } from "./chat"
import { useUiStore } from "./ui"

export type ShownFile = Exclude<FileView, { kind: "directory" }>

/** The lines a link named; `seq` is new on every open, so the view scrolls to them again. */
export type FileTarget = { line: number; endLine?: number; seq: number }

/** A file open in a Session's file view. A preview tab gives its place to the next file opened until it is kept. */
export type OpenFile = { path: string; preview: boolean; view?: ShownFile; error?: string; target?: FileTarget }

const WRAP_KEY = "dim-web.files.wrap"

function readWrap(): boolean {
  try {
    return localStorage.getItem(WRAP_KEY) !== "false"
  } catch {
    return true
  }
}

/** Each Session's open files, the tabs of the detail panel's file view, for this page load. */
export const useFilesStore = defineStore("files", () => {
  const chat = useChatStore()
  const ui = useUiStore()
  const bySession = ref<Record<string, OpenFile[]>>({})
  const active = ref<Record<string, string | undefined>>({})
  const wrap = ref(readWrap())
  let seq = 0

  watch(wrap, (value) => {
    try {
      localStorage.setItem(WRAP_KEY, String(value))
    } catch {
    }
  })

  function readable(sessionId: string): boolean {
    return chat.views[sessionId]?.methods.includes("session.files.read") === true
  }

  function update(sessionId: string, path: string, change: (file: OpenFile) => OpenFile): void {
    bySession.value = { ...bySession.value, [sessionId]: (bySession.value[sessionId] ?? []).map((file) => file.path === path ? change(file) : file) }
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
      next = files.map((file, at) => at === index ? { ...file, preview: file.preview && !keep, view, error: undefined, target: target ?? file.target } : file)
    } else {
      const opened: OpenFile = { path: view.path, preview: !keep, view, target }
      const preview = keep ? -1 : files.findIndex((file) => file.preview)
      next = preview === -1 ? [...files, opened] : files.map((file, at) => at === preview ? opened : file)
    }
    bySession.value = { ...bySession.value, [sessionId]: next }
    active.value = { ...active.value, [sessionId]: view.path }
    ui.openDetail("files")
    return true
  }

  /** Reads an open file again; an unchanged file keeps its view. */
  async function reload(sessionId: string, path: string): Promise<void> {
    try {
      const view = await chat.readFile(sessionId, path)
      update(sessionId, path, (file) => {
        if (view.kind === "directory") return file
        if (file.view !== undefined && file.error === undefined && file.view.modified === view.modified && file.view.size === view.size) return file
        return { ...file, view, error: undefined }
      })
    } catch (cause) {
      update(sessionId, path, (file) => ({ ...file, error: cause instanceof Error && cause.message ? cause.message : String(cause) }))
    }
  }

  function activate(sessionId: string, path: string | undefined): void {
    active.value = { ...active.value, [sessionId]: path }
  }

  function keepOpen(sessionId: string, path: string): void {
    update(sessionId, path, (file) => ({ ...file, preview: false }))
  }

  /** Closes a tab, activating its neighbour. */
  function close(sessionId: string, path: string): void {
    const files = bySession.value[sessionId] ?? []
    const index = files.findIndex((file) => file.path === path)
    const rest = files.filter((file) => file.path !== path)
    bySession.value = { ...bySession.value, [sessionId]: rest }
    if (active.value[sessionId] === path) activate(sessionId, (rest[index] ?? rest[index - 1])?.path)
  }

  return { bySession, active, wrap, readable, open, reload, activate, keepOpen, close }
})
