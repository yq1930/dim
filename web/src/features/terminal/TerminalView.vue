<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue"
import { useI18n } from "vue-i18n"
import { Terminal, type ITheme } from "@xterm/xterm"
import { FitAddon } from "@xterm/addon-fit"
import "@xterm/xterm/css/xterm.css"
import { ConnectionLost, RpcError } from "../../rpc/client"
import { readAck } from "../../rpc/read"
import { readTerminalChunk } from "../../rpc/terminal"
import { useConnectionStore } from "../../stores/connection"

/**
 * One server terminal drawn by xterm.js: it replays the output the server
 * kept, then long-polls `terminal.read`, and sends keystrokes and its size.
 */
const props = defineProps<{ terminalId: string }>()
const emit = defineEmits<{ exited: [terminalId: string]; gone: [terminalId: string] }>()
const { t } = useI18n()
const connection = useConnectionStore()
const host = ref<HTMLDivElement>()
let terminal: Terminal | undefined
let fit: FitAddon | undefined
let resizeObserver: ResizeObserver | undefined
let themeObserver: MutationObserver | undefined
let disposed = false
let cursor = 0
let writes = Promise.resolve()
let sentSize = ""
let frame = 0

const LIGHT: ITheme = { black: "#000000", red: "#cd3131", green: "#00bc00", yellow: "#949800", blue: "#0451a5", magenta: "#bc05bc", cyan: "#0598bc", white: "#555555", brightBlack: "#666666", brightRed: "#cd3131", brightGreen: "#14ce14", brightYellow: "#b5ba00", brightBlue: "#0451a5", brightMagenta: "#bc05bc", brightCyan: "#0598bc", brightWhite: "#a5a5a5" }
const DARK: ITheme = { black: "#000000", red: "#cd3131", green: "#0dbc79", yellow: "#e5e510", blue: "#2472c8", magenta: "#bc3fbc", cyan: "#11a8cd", white: "#e5e5e5", brightBlack: "#666666", brightRed: "#f14c4c", brightGreen: "#23d18b", brightYellow: "#f5f543", brightBlue: "#3b8eea", brightMagenta: "#d670d6", brightCyan: "#29b8db", brightWhite: "#e5e5e5" }

/** The theme's colors are oklch; xterm takes sRGB, so a canvas pixel converts them. */
const paint = document.createElement("canvas").getContext("2d", { willReadFrequently: true })
function rgb(color: string): string {
  if (paint === null) return color
  paint.clearRect(0, 0, 1, 1)
  paint.fillStyle = "#000"
  paint.fillStyle = color
  paint.fillRect(0, 0, 1, 1)
  return `#${[...paint.getImageData(0, 0, 1, 1).data.slice(0, 3)].map((channel) => channel.toString(16).padStart(2, "0")).join("")}`
}

function theme(element: HTMLElement): ITheme {
  const style = getComputedStyle(element)
  const color = (name: string) => rgb(style.getPropertyValue(name).trim())
  return {
    ...(document.documentElement.classList.contains("dark") ? DARK : LIGHT),
    background: color("--background"),
    foreground: color("--foreground"),
    cursor: color("--foreground"),
    cursorAccent: color("--background"),
    selectionBackground: `${color("--primary")}55`,
  }
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

function connected(): Promise<void> {
  if (connection.phase === "connected") return Promise.resolve()
  return new Promise((resolve) => {
    const stop = watch(() => connection.phase, (phase) => {
      if (phase !== "connected") return
      stop()
      resolve()
    })
  })
}

function fitSize(): void {
  if (terminal === undefined || fit === undefined || host.value?.offsetParent === null) return
  fit.fit()
  const size = `${terminal.cols}x${terminal.rows}`
  if (size === sentSize) return
  sentSize = size
  void connection.client.call("terminal.resize", { terminalId: props.terminalId, cols: terminal.cols, rows: terminal.rows }, readAck).catch(() => { sentSize = "" })
}

function send(data: string): void {
  const terminalId = props.terminalId
  writes = writes.then(() => connection.client.call("terminal.write", { terminalId, data }, readAck)).catch(() => undefined)
}

async function pump(): Promise<void> {
  while (!disposed) {
    if (connection.phase !== "connected") {
      await connected()
      sentSize = ""
      fitSize()
      continue
    }
    try {
      const chunk = await connection.client.call("terminal.read", { terminalId: props.terminalId, cursor, waitMs: 20_000 }, readTerminalChunk, 30_000)
      if (disposed || terminal === undefined) return
      if (chunk.reset) terminal.reset()
      if (chunk.data !== "") terminal.write(chunk.data)
      cursor = chunk.cursor
      if (chunk.exited) {
        const notice = chunk.exitCode === undefined ? t("terminal.ended") : t("terminal.exited", { code: chunk.exitCode })
        terminal.write(`\r\n\x1b[2m${notice}\x1b[0m\r\n`)
        emit("exited", props.terminalId)
        return
      }
    } catch (cause) {
      if (disposed) return
      if (cause instanceof RpcError && cause.appError === "terminal_unknown") {
        emit("gone", props.terminalId)
        return
      }
      await delay(cause instanceof ConnectionLost ? 300 : 1000)
    }
  }
}

onMounted(async () => {
  const element = host.value
  if (element === undefined) return
  const fontFamily = getComputedStyle(element).getPropertyValue("--font-mono").trim()
  // xterm measures its cell once; the web font has to be there first.
  await document.fonts.load(`12px ${fontFamily}`).catch(() => [])
  if (disposed) return
  terminal = new Terminal({ fontFamily, fontSize: 12, lineHeight: 1.2, cursorBlink: true, scrollback: 5000, theme: theme(element) })
  fit = new FitAddon()
  terminal.loadAddon(fit)
  // Ctrl+` is the app's shortcut that toggles this panel.
  terminal.attachCustomKeyEventHandler((event) => !(event.ctrlKey && event.code === "Backquote"))
  terminal.open(element)
  terminal.onData(send)
  fitSize()
  resizeObserver = new ResizeObserver(() => {
    cancelAnimationFrame(frame)
    frame = requestAnimationFrame(fitSize)
  })
  resizeObserver.observe(element)
  themeObserver = new MutationObserver(() => { if (terminal !== undefined) terminal.options.theme = theme(element) })
  themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] })
  terminal.focus()
  void pump()
})

onBeforeUnmount(() => {
  disposed = true
  cancelAnimationFrame(frame)
  resizeObserver?.disconnect()
  themeObserver?.disconnect()
  terminal?.dispose()
})
</script>

<template>
  <div class="terminal-frame">
    <div ref="host" class="terminal-host" />
  </div>
</template>

<style scoped>
.terminal-frame { display: flex; min-width: 0; min-height: 0; flex: 1; padding: var(--space-2) var(--space-1) var(--space-1) var(--space-3); background: var(--background); }
.terminal-host { min-width: 0; min-height: 0; flex: 1; }
</style>
