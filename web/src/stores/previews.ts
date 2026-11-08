import { ref, watch } from "vue"
import { defineStore } from "pinia"
import { RpcError } from "../rpc/client"
import { readAck } from "../rpc/read"
import { readOpenedTerminal, readTerminalChunk, readTerminals } from "../rpc/terminal"
import { showPreview } from "../desktop"
import { useChatStore } from "./chat"
import { useConnectionStore } from "./connection"
import { useUiStore } from "./ui"

/** A dev server of the project's `.claude/launch.json`, the file Claude Code reads its dev servers from. */
export type LaunchConfig = { name: string; command: string[]; port?: number; url?: string; cwd?: string }

/** A dev server running in a terminal of the Session, with its output as plain text. */
export type PreviewServer = { terminalId: string; name: string; url?: string; ready: boolean; exited: boolean; exitCode?: number; logs: string }

/** What the preview shows: an address, and the server it belongs to. `reload` is new on every reload. */
export type Preview = { url?: string; serverId?: string; reload: number }

const LAUNCH_FILE = ".claude/launch.json"
/** Logs past this many characters drop their beginning. */
const LOG_MAX = 200_000
/** A server whose output names no address is shown anyway after this long. */
const READY_AFTER_MS = 15_000
const LOCAL_HOSTS = new Set(["localhost", "127.0.0.1", "0.0.0.0", "[::1]", "[::]"])

/** `input` as an address on this computer (a bare `host:port` is taken as http), else undefined. */
export function localUrl(input: string): string | undefined {
  const text = input.trim()
  if (text === "") return undefined
  let url: URL
  try {
    url = new URL(/^[a-z][a-z0-9+.-]*:\/\//i.test(text) ? text : `http://${text}`)
  } catch {
    return undefined
  }
  if ((url.protocol !== "http:" && url.protocol !== "https:") || !LOCAL_HOSTS.has(url.hostname)) return undefined
  if (url.hostname !== "localhost" && url.hostname !== "127.0.0.1") url.hostname = "localhost"
  return url.href
}

/** The dev servers a launch file declares; entries without a name or a program are left out. */
export function parseLaunch(text: string): LaunchConfig[] {
  const value: unknown = JSON.parse(text)
  const entries = value !== null && typeof value === "object" && Array.isArray((value as { configurations?: unknown }).configurations) ? (value as { configurations: unknown[] }).configurations : []
  return entries.flatMap((entry) => {
    if (entry === null || typeof entry !== "object") return []
    const source = entry as Record<string, unknown>
    if (typeof source.name !== "string" || source.name.trim() === "" || typeof source.runtimeExecutable !== "string" || source.runtimeExecutable === "") return []
    const args = Array.isArray(source.runtimeArgs) ? source.runtimeArgs.filter((arg): arg is string => typeof arg === "string") : []
    const config: LaunchConfig = { name: source.name.trim().slice(0, 100), command: [source.runtimeExecutable, ...args] }
    if (typeof source.port === "number" && Number.isInteger(source.port) && source.port > 0 && source.port < 65536) config.port = source.port
    const url = typeof source.url === "string" ? localUrl(source.url) : undefined
    if (url !== undefined) config.url = url
    if (typeof source.cwd === "string" && source.cwd !== "") config.cwd = source.cwd
    return [config]
  })
}

/** Terminal output as plain text: colours and cursor movement gone, a line redrawn in place kept as last drawn. */
export function cleanLog(text: string): string {
  return text
    .replace(/\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g, "")
    .replace(/\x1b\[[0-9;?]*[ -/]*[@-~]/g, "")
    .replace(/\x1b[@-Z\\-_]/g, "")
    .replace(/\r\n/g, "\n")
    .split("\n")
    .map((line) => line.slice(line.lastIndexOf("\r") + 1))
    .join("\n")
}

/** The first address on this computer the output names, on `port` when one is given. */
export function servedUrl(logs: string, port?: number): string | undefined {
  for (const match of logs.matchAll(/https?:\/\/(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1?\])(?::\d+)?[^\s'"<>)\]]*/g)) {
    const url = localUrl(match[0])
    if (url !== undefined && (port === undefined || new URL(url).port === String(port))) return url
  }
  return undefined
}

function message(cause: unknown): string {
  return cause instanceof Error && cause.message ? cause.message : String(cause)
}

/** Each Session's dev servers and what its preview shows, for this page load; the servers' terminals outlive it. */
export const usePreviewsStore = defineStore("previews", () => {
  const chat = useChatStore()
  const connection = useConnectionStore()
  const ui = useUiStore()
  const configs = ref<Record<string, { list?: LaunchConfig[]; error?: string }>>({})
  const servers = ref<Record<string, PreviewServer[]>>({})
  const previews = ref<Record<string, Preview>>({})
  /** Whether the logs of the shown server are open below it. */
  const logsOpen = ref(false)
  const following = new Set<string>()

  // A restarted service has none of the terminals.
  watch(() => connection.hello?.instanceId, (next, previous) => {
    if (previous === undefined || next === previous) return
    servers.value = {}
    following.clear()
  })

  function update(sessionId: string, terminalId: string, change: (server: PreviewServer) => PreviewServer): void {
    servers.value = { ...servers.value, [sessionId]: (servers.value[sessionId] ?? []).map((server) => server.terminalId === terminalId ? change(server) : server) }
  }

  function find(sessionId: string, terminalId: string | undefined): PreviewServer | undefined {
    return servers.value[sessionId]?.find((server) => server.terminalId === terminalId)
  }

  /** Reads the project's launch file; a missing one declares no servers. */
  async function loadConfigs(sessionId: string): Promise<void> {
    try {
      const view = await chat.readFile(sessionId, LAUNCH_FILE)
      const list = view.kind === "text" ? parseLaunch(view.text) : []
      configs.value = { ...configs.value, [sessionId]: { list } }
    } catch (cause) {
      const missing = cause instanceof RpcError && cause.appError === "file_not_found"
      configs.value = { ...configs.value, [sessionId]: missing ? { list: [] } : { error: message(cause) } }
    }
  }

  /** Finds the Session's servers still running from before this page load and follows their output. */
  async function discover(sessionId: string): Promise<void> {
    if (!connection.supports("terminal.list")) return
    const listed = await connection.client.call("terminal.list", { owner: sessionId }, readTerminals)
    const known = servers.value[sessionId] ?? []
    const found = listed.filter((terminal) => terminal.command !== undefined && !known.some((server) => server.terminalId === terminal.terminalId))
    if (found.length === 0) return
    servers.value = { ...servers.value, [sessionId]: [...known, ...found.map((terminal) => ({ terminalId: terminal.terminalId, name: terminal.title, ready: false, exited: terminal.exited, logs: "" }))] }
    for (const terminal of found) void follow(sessionId, terminal.terminalId)
  }

  /** Shows `url` (an address on this computer) in the Session's preview, as part of the running server that serves its origin. */
  async function show(sessionId: string, url: string, serverId?: string): Promise<void> {
    await showPreview(url)
    const origin = new URL(url).origin
    const owner = serverId ?? servers.value[sessionId]?.find((server) => !server.exited && server.url !== undefined && new URL(server.url).origin === origin)?.terminalId
    previews.value = { ...previews.value, [sessionId]: { url, ...(owner === undefined ? {} : { serverId: owner }), reload: (previews.value[sessionId]?.reload ?? 0) + 1 } }
    ui.openDetail("preview")
  }

  function reload(sessionId: string): void {
    const preview = previews.value[sessionId]
    if (preview !== undefined) previews.value = { ...previews.value, [sessionId]: { ...preview, reload: preview.reload + 1 } }
  }

  /** Back to the server list; running servers keep running. */
  function leave(sessionId: string): void {
    previews.value = { ...previews.value, [sessionId]: { reload: (previews.value[sessionId]?.reload ?? 0) + 1 } }
  }

  /** Starts a launch file's server in a terminal of the Session and shows it once it serves. */
  async function start(sessionId: string, cwd: string, config: LaunchConfig): Promise<void> {
    const folder = config.cwd === undefined ? cwd : config.cwd.startsWith("/") ? config.cwd : `${cwd.replace(/\/+$/, "")}/${config.cwd}`
    const terminal = await connection.client.call("terminal.open", { owner: sessionId, cwd: folder, cols: 120, rows: 40, command: config.command, title: config.name }, readOpenedTerminal)
    const url = config.url ?? (config.port === undefined ? undefined : `http://localhost:${config.port}/`)
    servers.value = { ...servers.value, [sessionId]: [...(servers.value[sessionId] ?? []), { terminalId: terminal.terminalId, name: config.name, ...(url === undefined ? {} : { url }), ready: false, exited: false, logs: "" }] }
    void follow(sessionId, terminal.terminalId, config.port)
    if (url !== undefined) await show(sessionId, url, terminal.terminalId)
    else {
      previews.value = { ...previews.value, [sessionId]: { serverId: terminal.terminalId, reload: (previews.value[sessionId]?.reload ?? 0) + 1 } }
      logsOpen.value = true
    }
  }

  async function stop(sessionId: string, terminalId: string): Promise<void> {
    try {
      await connection.client.call("terminal.close", { terminalId }, readAck)
    } catch (cause) {
      if (!(cause instanceof RpcError && cause.appError === "terminal_unknown")) throw cause
    }
    update(sessionId, terminalId, (server) => ({ ...server, exited: true }))
  }

  /** Forgets a stopped server. */
  function dismiss(sessionId: string, terminalId: string): void {
    servers.value = { ...servers.value, [sessionId]: (servers.value[sessionId] ?? []).filter((server) => server.terminalId !== terminalId) }
    if (previews.value[sessionId]?.serverId === terminalId) leave(sessionId)
  }

  /** Reads a server's output until it exits, and shows it once the output names its address. */
  async function follow(sessionId: string, terminalId: string, port?: number): Promise<void> {
    if (following.has(terminalId)) return
    following.add(terminalId)
    const started = Date.now()
    let cursor = 0
    try {
      for (;;) {
        const server = find(sessionId, terminalId)
        if (server === undefined) return
        const chunk = await connection.client.call("terminal.read", { terminalId, cursor, waitMs: 5_000 }, readTerminalChunk, 15_000)
        cursor = chunk.cursor
        const logs = `${chunk.reset ? "" : server.logs}${cleanLog(chunk.data)}`.slice(-LOG_MAX)
        const named = server.ready ? undefined : servedUrl(logs, port)
        const ready = server.ready || named !== undefined || (server.url !== undefined && Date.now() - started > READY_AFTER_MS)
        const url = server.url ?? named
        update(sessionId, terminalId, (current) => ({ ...current, logs, ready, ...(url === undefined ? {} : { url }), exited: chunk.exited, ...(chunk.exitCode === undefined ? {} : { exitCode: chunk.exitCode }) }))
        if (ready && !server.ready && url !== undefined) {
          const preview = previews.value[sessionId]
          if (preview?.serverId === terminalId) await show(sessionId, url, terminalId)
        }
        if (chunk.exited) return
      }
    } catch (cause) {
      if (!(cause instanceof RpcError && cause.appError === "terminal_unknown")) update(sessionId, terminalId, (server) => ({ ...server, logs: `${server.logs}\n${message(cause)}\n`.slice(-LOG_MAX) }))
      update(sessionId, terminalId, (server) => ({ ...server, exited: true }))
    } finally {
      following.delete(terminalId)
    }
  }

  return { configs, servers, previews, logsOpen, loadConfigs, discover, show, reload, leave, start, stop, dismiss }
})
