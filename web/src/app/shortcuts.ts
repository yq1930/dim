import { canChooseWorkspace as inDesktop } from "../desktop"
import type { TranslationKey } from "../i18n"

/**
 * The app's keyboard shortcuts as one table in the shape of Claude Code's: each
 * binding names the command it runs and its keys, with `cmd` read as Ctrl off
 * the Mac. The shell runs the commands; the shortcuts dialog lists them beside
 * the keys components handle themselves.
 */
export type ShortcutCommand =
  | "showShortcuts" | "toggleSidebar" | "newSession" | "openSettings" | "cycleTranscriptMode"
  | "toggleDiff" | "toggleFiles" | "togglePreview" | "toggleTerminal" | "closePane" | "toggleExpandPane"
  | "openModeMenu" | "openModelMenu"

type Binding = {
  command: ShortcutCommand
  /** Modifiers and the key, joined by "+". */
  key: string
  /** The physical key, matched unless the key types the binding's character. */
  code: string
  /** "char": by the character the key types, so letters follow the keyboard layout. */
  matchBy?: "char"
  /** Only on the Mac, or only elsewhere. */
  mac?: boolean
  /** Only in the desktop app, or only in a browser, which keeps some keys for itself. */
  when?: "desktop" | "browser"
}

export const SHORTCUTS: readonly Binding[] = [
  { command: "showShortcuts", key: "cmd+/", code: "Slash", matchBy: "char" },
  { command: "toggleSidebar", key: "cmd+b", code: "KeyB", matchBy: "char" },
  { command: "newSession", key: "cmd+n", code: "KeyN", matchBy: "char", when: "desktop" },
  { command: "newSession", key: "cmd+shift+o", code: "KeyO", matchBy: "char", when: "browser" },
  { command: "openSettings", key: "cmd+,", code: "Comma", matchBy: "char", when: "desktop" },
  { command: "cycleTranscriptMode", key: "ctrl+o", code: "KeyO", matchBy: "char" },
  { command: "toggleDiff", key: "cmd+shift+d", code: "KeyD", matchBy: "char", when: "desktop" },
  { command: "toggleDiff", key: "ctrl+shift+d", code: "KeyD", matchBy: "char", when: "browser" },
  { command: "toggleFiles", key: "cmd+shift+f", code: "KeyF", matchBy: "char" },
  { command: "togglePreview", key: "cmd+shift+p", code: "KeyP", matchBy: "char", when: "desktop" },
  { command: "togglePreview", key: "cmd+alt+p", code: "KeyP", matchBy: "char", when: "browser" },
  { command: "toggleTerminal", key: "cmd+j", code: "KeyJ", matchBy: "char", when: "desktop", mac: true },
  { command: "toggleTerminal", key: "ctrl+`", code: "Backquote" },
  { command: "closePane", key: "cmd+\\", code: "Backslash" },
  { command: "toggleExpandPane", key: "cmd+shift+\\", code: "Backslash" },
  { command: "openModeMenu", key: "cmd+shift+m", code: "KeyM", matchBy: "char", when: "desktop" },
  { command: "openModeMenu", key: "cmd+alt+m", code: "KeyM", matchBy: "char" },
  { command: "openModelMenu", key: "cmd+shift+i", code: "KeyI", matchBy: "char" },
]

export type ShortcutContext = { mac: boolean; desktop: boolean }

export function shortcutContext(): ShortcutContext {
  return { mac: typeof navigator !== "undefined" && navigator.userAgent.includes("Mac"), desktop: inDesktop }
}

type Modifiers = { cmd: boolean; ctrl: boolean; alt: boolean; shift: boolean }

function modifiers(key: string): Modifiers {
  const names = key.split("+").slice(0, -1)
  return { cmd: names.includes("cmd"), ctrl: names.includes("ctrl"), alt: names.includes("alt"), shift: names.includes("shift") }
}

function applies(binding: Binding, context: ShortcutContext): boolean {
  return (binding.mac === undefined || binding.mac === context.mac) && (binding.when === undefined || (binding.when === "desktop") === context.desktop)
}

type KeyPress = Pick<KeyboardEvent, "key" | "code" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey">

function matches(event: KeyPress, binding: Binding, mac: boolean): boolean {
  const wanted = modifiers(binding.key)
  if (event.metaKey !== (mac && wanted.cmd) || event.ctrlKey !== (wanted.ctrl || (!mac && wanted.cmd)) || event.altKey !== wanted.alt || event.shiftKey !== wanted.shift) return false
  const last = binding.key.split("+").at(-1)
  // A key that types no ASCII character (another script, or Option on the Mac) is matched where it sits;
  // an event without a physical key (a virtual keyboard) by its character.
  if ((binding.matchBy === "char" && /^[ -~]$/.test(event.key)) || event.code === "") return event.key.toLowerCase() === last
  return event.code === binding.code
}

/** The command a key press runs, if any. */
export function matchShortcut(event: KeyPress, context: ShortcutContext): ShortcutCommand | undefined {
  return SHORTCUTS.find((binding) => applies(binding, context) && matches(event, binding, context.mac))?.command
}

/** The keys that run `command` here, the preferred first. */
export function shortcutKeys(command: ShortcutCommand, context: ShortcutContext): string[] {
  return SHORTCUTS.filter((binding) => binding.command === command && applies(binding, context)).map((binding) => binding.key)
}

const MAC_MODIFIERS = [["ctrl", "⌃"], ["alt", "⌥"], ["shift", "⇧"], ["cmd", "⌘"]] as const
const KEY_NAMES: Record<string, string> = { enter: "Enter", esc: "Esc", tab: "Tab", up: "↑", down: "↓" }

/** A key combination as the key caps to show, in the platform's order and names. */
export function keyCaps(key: string, mac: boolean): string[] {
  const wanted = modifiers(key)
  const last = key.split("+").at(-1) ?? ""
  const held = mac
    ? MAC_MODIFIERS.filter(([name]) => wanted[name]).map(([, cap]) => cap)
    : [...(wanted.ctrl || wanted.cmd ? ["Ctrl"] : []), ...(wanted.alt ? ["Alt"] : []), ...(wanted.shift ? ["Shift"] : [])]
  return [...held, KEY_NAMES[last] ?? last.toUpperCase()]
}

/** A key combination as one piece of text: ⌘⇧D on the Mac, Ctrl+Shift+D elsewhere. */
export function keyText(key: string, mac: boolean): string {
  return keyCaps(key, mac).join(mac ? "" : "+")
}

/** The preferred keys of `command` here as text, if it has any. */
export function shortcutText(command: ShortcutCommand): string | undefined {
  const context = shortcutContext()
  const key = shortcutKeys(command, context)[0]
  return key === undefined ? undefined : keyText(key, context.mac)
}

/** A tooltip naming a control and the keys of the command it runs. */
export function withKeys(text: string, command: ShortcutCommand): { text: string; keys?: string } {
  const keys = shortcutText(command)
  return keys === undefined ? { text } : { text, keys }
}

export type ShortcutGroup = "general" | "panes" | "composer"
export const SHORTCUT_GROUPS: readonly ShortcutGroup[] = ["general", "panes", "composer"]

/** A row of the shortcuts dialog: a command of the table, or the keys a component handles itself. */
export type ShortcutHelp = { group: ShortcutGroup; label: TranslationKey } & ({ command: ShortcutCommand } | { keys: readonly string[] })

export const SHORTCUT_HELP: readonly ShortcutHelp[] = [
  { group: "general", label: "shortcuts.newSession", command: "newSession" },
  { group: "general", label: "shortcuts.toggleSidebar", command: "toggleSidebar" },
  { group: "general", label: "shortcuts.openSettings", command: "openSettings" },
  { group: "general", label: "shortcuts.title", command: "showShortcuts" },
  { group: "general", label: "shortcuts.transcriptView", command: "cycleTranscriptMode" },
  { group: "general", label: "shortcuts.interrupt", keys: ["esc"] },
  { group: "general", label: "shortcuts.allow", keys: ["enter", "y"] },
  { group: "general", label: "shortcuts.deny", keys: ["esc", "n"] },
  { group: "panes", label: "shortcuts.toggleChanges", command: "toggleDiff" },
  { group: "panes", label: "shortcuts.toggleFiles", command: "toggleFiles" },
  { group: "panes", label: "shortcuts.togglePreview", command: "togglePreview" },
  { group: "panes", label: "shortcuts.toggleTerminal", command: "toggleTerminal" },
  { group: "panes", label: "shortcuts.closePane", command: "closePane" },
  { group: "panes", label: "shortcuts.expandPane", command: "toggleExpandPane" },
  { group: "panes", label: "shortcuts.saveFile", keys: ["cmd+s"] },
  { group: "composer", label: "shortcuts.send", keys: ["enter"] },
  { group: "composer", label: "shortcuts.newLine", keys: ["shift+enter"] },
  { group: "composer", label: "shortcuts.modeMenu", command: "openModeMenu" },
  { group: "composer", label: "shortcuts.cycleMode", keys: ["shift+tab"] },
  { group: "composer", label: "shortcuts.modelMenu", command: "openModelMenu" },
  { group: "composer", label: "shortcuts.recall", keys: ["up"] },
]
