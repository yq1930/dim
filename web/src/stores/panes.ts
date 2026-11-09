import { ref, watch } from "vue"
import { defineStore } from "pinia"
import { TRANSCRIPT_VIEWS, usePreferencesStore, type TranscriptView } from "./preferences"

/** What the right panel shows for a Session. */
export type DetailView = "details" | "changes" | "files" | "preview" | "plan" | "tasks" | "members" | "terminal" | "git" | "lines"
const DETAIL_VIEWS: readonly DetailView[] = ["details", "changes", "files", "preview", "plan", "tasks", "members", "terminal", "git", "lines"]

/**
 * A Session's panes, as Claude Code remembers each session's tiles: whether the
 * right panel is open, the view it shows and whether it covers the transcript;
 * the files open in it and the dev server its preview shows.
 */
export type SessionPanes = {
  open: boolean
  view: DetailView
  expanded: boolean
  /** The file tabs' paths in order, and the one shown. */
  files?: { paths: string[]; active?: string }
  /** The address the preview shows, and the terminal of the server that serves it. */
  preview?: { url: string; serverId: string }
}

/** What the keyboard asks of a view itself: the terminal's tabs, the file view's list and finder. */
export type PaneCommand = "newTerminal" | "nextTerminal" | "prevTerminal" | "toggleFileList" | "goToFile"
type PaneRequest = { sessionId: string; view: DetailView; command: PaneCommand }

const STORAGE_KEY = "dim-web.panes.v1"
/** Sessions whose panes and transcript view are remembered; past this the least recently changed are forgotten. */
const SESSIONS_MAX = 200
const FILES_MAX = 20
const CLOSED: SessionPanes = { open: false, view: "changes", expanded: false }

type Persisted = { layouts: Record<string, SessionPanes>; transcripts: Record<string, TranscriptView> }

function record(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : undefined
}

function readPanes(value: unknown): SessionPanes | undefined {
  const stored = record(value)
  if (stored === undefined) return undefined
  const panes: SessionPanes = {
    open: stored.open === true,
    view: DETAIL_VIEWS.find((view) => view === stored.view) ?? "changes",
    expanded: stored.expanded === true,
  }
  const files = record(stored.files)
  if (files !== undefined && Array.isArray(files.paths)) {
    const paths = files.paths.filter((path): path is string => typeof path === "string").slice(0, FILES_MAX)
    panes.files = { paths, ...(typeof files.active === "string" && paths.includes(files.active) ? { active: files.active } : {}) }
  }
  const preview = record(stored.preview)
  if (typeof preview?.url === "string" && typeof preview.serverId === "string") panes.preview = { url: preview.url, serverId: preview.serverId }
  return panes
}

/** localStorage is outside the app: read it once, keeping only well-formed entries. */
function readPersisted(): Persisted {
  try {
    const stored = record(JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "null"))
    const layouts = Object.entries(record(stored?.layouts) ?? {}).flatMap(([id, value]) => {
      const panes = readPanes(value)
      return panes === undefined ? [] : [[id, panes] as const]
    })
    const transcripts = Object.entries(record(stored?.transcripts) ?? {}).filter((entry): entry is [string, TranscriptView] => TRANSCRIPT_VIEWS.includes(entry[1] as TranscriptView))
    return { layouts: Object.fromEntries(layouts), transcripts: Object.fromEntries(transcripts) }
  } catch {
    return { layouts: {}, transcripts: {} }
  }
}

/** `entries` with `id` set last, the oldest dropped past the cap; `value` undefined removes it. */
function remember<T>(entries: Record<string, T>, id: string, value: T | undefined): Record<string, T> {
  const { [id]: _, ...rest } = entries
  if (value === undefined) return rest
  const kept = Object.entries(rest)
  return Object.fromEntries([...kept.slice(Math.max(0, kept.length - SESSIONS_MAX + 1)), [id, value]])
}

/** Each Session's panes and transcript view, kept in this browser; its line tabs, for this page load. */
export const usePanesStore = defineStore("panes", () => {
  const persisted = readPersisted()
  const layouts = ref(persisted.layouts)
  const transcripts = ref(persisted.transcripts)
  /** The lines open as tabs in each Session's Lines view, and the one shown (none: the lines tree). */
  const lineTabs = ref<Record<string, string[]>>({})
  const activeLine = ref<Record<string, string | undefined>>({})
  /** The member each Session's Members view shows, by its request event id (none: the list). */
  const activeMember = ref<Record<string, string | undefined>>({})
  /** Whether the operator last pressed or focused the right panel rather than a transcript. */
  const focused = ref(false)
  /** A command for a view, waiting until the view shows to take it. */
  const request = ref<PaneRequest>()

  watch([layouts, transcripts], () => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify({ layouts: layouts.value, transcripts: transcripts.value }))
    } catch { /* best effort: full or disabled storage keeps the panes for this page load */ }
  })

  function of(sessionId: string): SessionPanes {
    return layouts.value[sessionId] ?? CLOSED
  }

  function update(sessionId: string, change: (panes: SessionPanes) => SessionPanes): void {
    layouts.value = remember(layouts.value, sessionId, change(of(sessionId)))
  }

  function show(sessionId: string, view: DetailView): void {
    update(sessionId, (panes) => ({ ...panes, open: true, view }))
    focused.value = true
  }

  /** Shows `view`, or closes the panel where it already shows it. */
  function toggle(sessionId: string, view: DetailView): void {
    const panes = of(sessionId)
    if (panes.open && panes.view === view) close(sessionId)
    else show(sessionId, view)
  }

  function close(sessionId: string): void {
    update(sessionId, (panes) => ({ ...panes, open: false, expanded: false }))
    focused.value = false
  }

  /** Shows `view` and hands it `command`. */
  function send(sessionId: string, view: DetailView, command: PaneCommand): void {
    show(sessionId, view)
    request.value = { sessionId, view, command }
  }

  /** The command waiting for `view` of the Session, taken so it runs once. */
  function take(sessionId: string, view: DetailView): PaneCommand | undefined {
    const pending = request.value
    if (pending?.sessionId !== sessionId || pending.view !== view) return undefined
    request.value = undefined
    return pending.command
  }

  function toggleExpanded(sessionId: string): void {
    update(sessionId, (panes) => ({ ...panes, expanded: !panes.expanded }))
  }

  /** The Lines view at its tree. */
  function openLines(sessionId: string): void {
    show(sessionId, "lines")
    activeLine.value = { ...activeLine.value, [sessionId]: undefined }
  }

  function toggleLines(sessionId: string): void {
    const panes = of(sessionId)
    if (panes.open && panes.view === "lines" && activeLine.value[sessionId] === undefined) close(sessionId)
    else openLines(sessionId)
  }

  /** Shows a line in its tab, opening one for it. */
  function openLine(sessionId: string, branchId: string): void {
    const tabs = lineTabs.value[sessionId] ?? []
    if (!tabs.includes(branchId)) lineTabs.value = { ...lineTabs.value, [sessionId]: [...tabs, branchId] }
    activeLine.value = { ...activeLine.value, [sessionId]: branchId }
    show(sessionId, "lines")
  }

  /** Shows the Members view at one member's work, or at the list. */
  function openMember(sessionId: string, requestId: string | undefined): void {
    activeMember.value = { ...activeMember.value, [sessionId]: requestId }
    show(sessionId, "members")
  }

  function closeLineTab(sessionId: string, branchId: string): void {
    const tabs = (lineTabs.value[sessionId] ?? []).filter((id) => id !== branchId)
    lineTabs.value = { ...lineTabs.value, [sessionId]: tabs }
    if (activeLine.value[sessionId] === branchId) activeLine.value = { ...activeLine.value, [sessionId]: tabs.at(-1) }
  }

  /** A closed Session keeps no line tabs and no open panel. */
  function reset(sessionId: string): void {
    lineTabs.value = { ...lineTabs.value, [sessionId]: [] }
    activeLine.value = { ...activeLine.value, [sessionId]: undefined }
    if (of(sessionId).open) close(sessionId)
  }

  function setFiles(sessionId: string, paths: string[], active: string | undefined): void {
    const kept = paths.slice(-FILES_MAX)
    update(sessionId, (panes) => ({ ...panes, files: { paths: kept, ...(active !== undefined && kept.includes(active) ? { active } : {}) } }))
  }

  function setPreview(sessionId: string, preview: SessionPanes["preview"]): void {
    update(sessionId, (panes) => {
      const { preview: _, ...rest } = panes
      return preview === undefined ? rest : { ...rest, preview }
    })
  }

  function transcriptOf(sessionId: string): TranscriptView {
    return transcripts.value[sessionId] ?? usePreferencesStore().transcriptView
  }

  /** Sets one Session's view; a Session on the default view keeps no entry. */
  function setTranscript(sessionId: string, view: TranscriptView): void {
    transcripts.value = remember(transcripts.value, sessionId, view === usePreferencesStore().transcriptView ? undefined : view)
  }

  /** Steps a Session to the next view, as Ctrl+O does in Claude Code. */
  function cycleTranscript(sessionId: string): TranscriptView {
    const next = TRANSCRIPT_VIEWS[(TRANSCRIPT_VIEWS.indexOf(transcriptOf(sessionId)) + 1) % TRANSCRIPT_VIEWS.length]!
    setTranscript(sessionId, next)
    return next
  }

  /** Makes a Session's view the default, which that Session then follows. */
  function makeTranscriptDefault(sessionId: string): void {
    usePreferencesStore().transcriptView = transcriptOf(sessionId)
    transcripts.value = remember(transcripts.value, sessionId, undefined)
  }

  return {
    layouts, transcripts, lineTabs, activeLine, activeMember, focused, request,
    of, show, send, take, toggle, close, toggleExpanded, openLines, toggleLines, openLine, openMember, closeLineTab, reset, setFiles, setPreview,
    transcriptOf, setTranscript, cycleTranscript, makeTranscriptDefault,
  }
})
