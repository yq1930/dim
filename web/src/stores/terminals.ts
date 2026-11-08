import { computed, ref, watch } from "vue"
import { defineStore } from "pinia"
import { RpcError } from "../rpc/client"
import { readAck } from "../rpc/read"
import { readOpenedTerminal, readTerminals, type TerminalSummary } from "../rpc/terminal"
import { useConnectionStore } from "./connection"

/** Each Session's terminals, the tabs of its terminal panel; the server keeps them across reloads. */
export const useTerminalsStore = defineStore("terminals", () => {
  const connection = useConnectionStore()
  const bySession = ref<Record<string, TerminalSummary[]>>({})
  const active = ref<Record<string, string | undefined>>({})
  const available = computed(() => connection.supports("terminal.open"))
  let loads = new Map<string, Promise<void>>()

  // Terminals belong to the server process; a restarted one has none of them.
  watch(() => connection.hello?.instanceId, (next, previous) => {
    if (previous === undefined || next === previous) return
    loads = new Map()
    bySession.value = {}
    active.value = {}
  })

  function load(sessionId: string): Promise<void> {
    let pending = loads.get(sessionId)
    if (pending === undefined) {
      pending = connection.client.call("terminal.list", { owner: sessionId }, readTerminals).then((terminals) => {
        bySession.value = { ...bySession.value, [sessionId]: terminals }
        if (!terminals.some((terminal) => terminal.terminalId === active.value[sessionId])) activate(sessionId, terminals.at(-1)?.terminalId)
      }, (cause: unknown) => {
        loads.delete(sessionId)
        throw cause
      })
      loads.set(sessionId, pending)
    }
    return pending
  }

  async function open(sessionId: string, cwd: string): Promise<void> {
    const terminal = await connection.client.call("terminal.open", { owner: sessionId, cwd, cols: 80, rows: 24 }, readOpenedTerminal)
    bySession.value = { ...bySession.value, [sessionId]: [...(bySession.value[sessionId] ?? []), terminal] }
    activate(sessionId, terminal.terminalId)
  }

  function activate(sessionId: string, terminalId: string | undefined): void {
    active.value = { ...active.value, [sessionId]: terminalId }
  }

  function markExited(sessionId: string, terminalId: string): void {
    bySession.value = { ...bySession.value, [sessionId]: (bySession.value[sessionId] ?? []).map((terminal) => terminal.terminalId === terminalId ? { ...terminal, exited: true } : terminal) }
  }

  /** Drops a terminal from its Session's tabs, activating its neighbour. */
  function forget(sessionId: string, terminalId: string): void {
    const terminals = bySession.value[sessionId] ?? []
    const index = terminals.findIndex((terminal) => terminal.terminalId === terminalId)
    const rest = terminals.filter((terminal) => terminal.terminalId !== terminalId)
    bySession.value = { ...bySession.value, [sessionId]: rest }
    if (active.value[sessionId] === terminalId) activate(sessionId, (rest[index] ?? rest[index - 1])?.terminalId)
  }

  async function close(sessionId: string, terminalId: string): Promise<void> {
    try {
      await connection.client.call("terminal.close", { terminalId }, readAck)
    } catch (cause) {
      if (!(cause instanceof RpcError && cause.appError === "terminal_unknown")) throw cause
    }
    forget(sessionId, terminalId)
  }

  return { bySession, active, available, load, open, activate, markExited, forget, close }
})
