import { computed, ref, watch } from "vue"
import { defineStore } from "pinia"
import { approvalKeyboardBranch } from "../features/lines/line-state"
import { useAppearanceStore } from "./appearance"

export type ShellMode = "compact" | "regular" | "wide"

/** What the right detail panel shows for the Session on screen. */
export type DetailView = "changes" | "files" | "preview" | "plan" | "tasks" | "terminal" | "git" | "lines"
const DETAIL_VIEWS: readonly DetailView[] = ["changes", "files", "preview", "plan", "tasks", "terminal", "git", "lines"]

/** The pane a Session shows in: the routed one, or the one beside it in split view. */
export type Pane = "main" | "split"

/** A review comment on one line of a change, kept until it is put into the composer. */
export type ReviewNote = {
  /** The change and line it is on, unique within the Session. */
  key: string
  /** The file as the panel shows it, under the workspace when it lies there. */
  file: string
  line?: number
  removed: boolean
  code: string
  note: string
}

/** Text a panel asks the Session's main composer to take. */
export type ComposerInsert = { sessionId: string; text: string; id: number }

/** A menu of a pane's composer that the keyboard asked to open. */
export type ComposerMenuRequest = { pane: Pane; menu: "mode" | "model"; id: number }

export type NewLineRequest = {
  sessionId: string
  parentBranchId: string
  at: number
  text: string
  before?: string
}

export type EndLineRequest = {
  sessionId: string
  branchId: string
  purpose: string
}

const STORAGE_KEY = "dim-web.ui.v1"
const DETAIL_MIN = 320
const DETAIL_MAX = 720
/** Split view needs room for two transcripts beside the sidebar. */
const SPLIT_MIN_WIDTH = 1100

/** Layout state remembered in this browser. */
type Persisted = {
  sidebarCollapsed: boolean
  collapsedWorkspaces: string[]
  lastSessionId: string | undefined
  detailOpen: boolean
  detailView: DetailView
  detailWidth: number
  /** Sessions shown first in the sidebar, in the order they were pinned. */
  pinnedSessions: string[]
  splitSessionId: string | undefined
}

/** localStorage is outside the app: read it once, keeping only well-formed fields. */
function readPersisted(): Persisted {
  const fallback: Persisted = { sidebarCollapsed: false, collapsedWorkspaces: [], lastSessionId: undefined, detailOpen: false, detailView: "lines", detailWidth: 420, pinnedSessions: [], splitSessionId: undefined }
  try {
    const value: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "null")
    if (value === null || typeof value !== "object") return fallback
    const stored = value as Record<string, unknown>
    const width = typeof stored.detailWidth === "number" && Number.isFinite(stored.detailWidth)
      ? Math.min(DETAIL_MAX, Math.max(DETAIL_MIN, stored.detailWidth))
      : fallback.detailWidth
    return {
      sidebarCollapsed: stored.sidebarCollapsed === true,
      collapsedWorkspaces: Array.isArray(stored.collapsedWorkspaces) ? stored.collapsedWorkspaces.filter((item): item is string => typeof item === "string") : [],
      lastSessionId: typeof stored.lastSessionId === "string" ? stored.lastSessionId : undefined,
      detailOpen: stored.detailOpen === true,
      detailView: DETAIL_VIEWS.find((view) => view === stored.detailView) ?? "lines",
      detailWidth: width,
      pinnedSessions: Array.isArray(stored.pinnedSessions) ? stored.pinnedSessions.filter((item): item is string => typeof item === "string") : [],
      splitSessionId: typeof stored.splitSessionId === "string" ? stored.splitSessionId : undefined,
    }
  } catch {
    return fallback
  }
}

/** Layout persistence is best-effort: full or disabled localStorage is ignored. */
export const useUiStore = defineStore("ui", () => {
  const persisted = readPersisted()
  const sidebarCollapsed = ref(persisted.sidebarCollapsed)
  const collapsedWorkspaces = ref(persisted.collapsedWorkspaces)
  const lastSessionId = ref(persisted.lastSessionId)
  const drawerOpen = ref(false)
  const shellWidth = ref(typeof window !== "undefined" && window.innerWidth > 0 ? window.innerWidth : 1200)
  const shellMode = computed<ShellMode>(() => shellWidth.value < 720 ? "compact" : shellWidth.value < 960 ? "regular" : "wide")
  const detailOpen = ref(persisted.detailOpen)
  const detailView = ref<DetailView>(persisted.detailView)
  const detailWidth = ref(persisted.detailWidth)
  const pinnedSessions = ref(persisted.pinnedSessions)
  const detailExpanded = ref(false)
  const detailFocused = ref(false)
  const lineTabs = ref<Record<string, string[]>>({})
  const activeLineTab = ref<Record<string, string | undefined>>({})
  const newLine = ref<NewLineRequest | undefined>()
  const endingLine = ref<EndLineRequest | undefined>()
  /** The Session on screen, if any; a run that ends elsewhere leaves its Session unread. */
  const viewingSessionId = ref<string>()
  /** Sessions whose run ended while another page was shown; kept for this page load only. */
  const unread = ref<string[]>([])
  /** Review comments per Session, for this page load only. */
  const reviewNotes = ref<Record<string, ReviewNote[]>>({})
  const composerInsert = ref<ComposerInsert>()
  let insertSeq = 0
  const composerMenu = ref<ComposerMenuRequest>()
  /** Whether the keyboard shortcuts dialog is open. */
  const shortcutsOpen = ref(false)
  /** The Session shown beside the routed one, when the shell is wide enough. */
  const splitSessionId = ref(persisted.splitSessionId)
  const splitAvailable = computed(() => shellWidth.value >= SPLIT_MIN_WIDTH)
  /** The pane the operator last pressed or focused; it takes the keyboard's approvals. */
  const activePane = ref<Pane>("main")

  watch([sidebarCollapsed, collapsedWorkspaces, lastSessionId, detailOpen, detailView, detailWidth, pinnedSessions, splitSessionId], () => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify({
        sidebarCollapsed: sidebarCollapsed.value,
        collapsedWorkspaces: collapsedWorkspaces.value,
        lastSessionId: lastSessionId.value,
        detailOpen: detailOpen.value,
        detailView: detailView.value,
        detailWidth: detailWidth.value,
        pinnedSessions: pinnedSessions.value,
        splitSessionId: splitSessionId.value,
      }))
    } catch {
    }
  }, { deep: true })

  useAppearanceStore()

  function setShellWidth(width: number): void {
    shellWidth.value = width
    if (shellMode.value !== "compact") drawerOpen.value = false
  }

  function isWorkspaceExpanded(cwd: string): boolean {
    return !collapsedWorkspaces.value.includes(cwd)
  }

  function togglePinned(sessionId: string): void {
    pinnedSessions.value = pinnedSessions.value.includes(sessionId)
      ? pinnedSessions.value.filter((id) => id !== sessionId)
      : [...pinnedSessions.value, sessionId]
  }

  function unpin(sessionId: string): void {
    pinnedSessions.value = pinnedSessions.value.filter((id) => id !== sessionId)
  }

  function toggleWorkspace(cwd: string): void {
    collapsedWorkspaces.value = isWorkspaceExpanded(cwd)
      ? [...collapsedWorkspaces.value, cwd]
      : collapsedWorkspaces.value.filter((item) => item !== cwd)
  }

  function setDetailWidth(width: number): void {
    detailWidth.value = Math.min(DETAIL_MAX, Math.max(DETAIL_MIN, Math.round(width)))
  }

  function viewSession(sessionId: string | undefined): void {
    viewingSessionId.value = sessionId
    if (sessionId !== undefined && unread.value.includes(sessionId)) unread.value = unread.value.filter((id) => id !== sessionId)
  }

  function markUnread(sessionId: string): void {
    if (sessionId !== viewingSessionId.value && sessionId !== splitSessionId.value && !unread.value.includes(sessionId)) unread.value = [...unread.value, sessionId]
  }

  function openSplit(sessionId: string): void {
    splitSessionId.value = sessionId
    activePane.value = "split"
    if (unread.value.includes(sessionId)) unread.value = unread.value.filter((id) => id !== sessionId)
  }

  function closeSplit(): void {
    splitSessionId.value = undefined
    activePane.value = "main"
  }

  function addReviewNote(sessionId: string, note: ReviewNote): void {
    const notes = (reviewNotes.value[sessionId] ?? []).filter((existing) => existing.key !== note.key)
    reviewNotes.value = { ...reviewNotes.value, [sessionId]: [...notes, note] }
  }

  function removeReviewNote(sessionId: string, key: string): void {
    reviewNotes.value = { ...reviewNotes.value, [sessionId]: (reviewNotes.value[sessionId] ?? []).filter((note) => note.key !== key) }
  }

  function clearReviewNotes(sessionId: string): void {
    reviewNotes.value = { ...reviewNotes.value, [sessionId]: [] }
  }

  function requestComposerInsert(sessionId: string, text: string): void {
    composerInsert.value = { sessionId, text, id: ++insertSeq }
  }

  function requestComposerMenu(pane: Pane, menu: ComposerMenuRequest["menu"]): void {
    composerMenu.value = { pane, menu, id: ++insertSeq }
  }

  function openDetail(view: DetailView): void {
    detailView.value = view
    detailOpen.value = true
    detailFocused.value = true
  }

  function toggleDetail(view: DetailView): void {
    if (detailOpen.value && detailView.value === view) {
      detailOpen.value = false
      detailFocused.value = false
    } else openDetail(view)
  }

  function openLines(sessionId: string): void {
    openDetail("lines")
    activeLineTab.value = { ...activeLineTab.value, [sessionId]: undefined }
  }

  function toggleLines(sessionId: string): void {
    if (detailOpen.value && detailView.value === "lines" && activeLineTab.value[sessionId] === undefined) {
      detailOpen.value = false
      detailFocused.value = false
    } else openLines(sessionId)
  }

  function openLine(sessionId: string, branchId: string): void {
    const tabs = lineTabs.value[sessionId] ?? []
    if (!tabs.includes(branchId)) lineTabs.value = { ...lineTabs.value, [sessionId]: [...tabs, branchId] }
    activeLineTab.value = { ...activeLineTab.value, [sessionId]: branchId }
    openDetail("lines")
  }

  function activateLine(sessionId: string, branchId: string): void {
    if (!(lineTabs.value[sessionId] ?? []).includes(branchId)) {
      openLine(sessionId, branchId)
      return
    }
    activeLineTab.value = { ...activeLineTab.value, [sessionId]: branchId }
    openDetail("lines")
  }

  function closeLineTab(sessionId: string, branchId: string): void {
    const tabs = (lineTabs.value[sessionId] ?? []).filter((id) => id !== branchId)
    lineTabs.value = { ...lineTabs.value, [sessionId]: tabs }
    if (activeLineTab.value[sessionId] === branchId) {
      activeLineTab.value = { ...activeLineTab.value, [sessionId]: tabs.at(-1) }
    }
  }

  function closeDetail(): void {
    detailOpen.value = false
    detailExpanded.value = false
    detailFocused.value = false
  }

  function approvalKeyboard(sessionId: string, branchId: string, approvals: readonly { branchId: string }[]): boolean {
    return approvalKeyboardBranch(approvals, {
      detailOpen: detailOpen.value,
      detailExpanded: detailExpanded.value,
      shellWidth: shellWidth.value,
      activeLine: detailView.value === "lines" ? activeLineTab.value[sessionId] : undefined,
      detailFocused: detailFocused.value,
    }) === branchId
  }

  function requestNewLine(request: NewLineRequest): void {
    newLine.value = request
  }

  function closeNewLine(): void {
    newLine.value = undefined
  }

  function requestEndLine(request: EndLineRequest): void {
    endingLine.value = request
  }

  function closeEndLine(): void {
    endingLine.value = undefined
  }

  function resetSessionTabs(sessionId: string): void {
    lineTabs.value = { ...lineTabs.value, [sessionId]: [] }
    activeLineTab.value = { ...activeLineTab.value, [sessionId]: undefined }
    if (newLine.value?.sessionId === sessionId) newLine.value = undefined
    if (endingLine.value?.sessionId === sessionId) endingLine.value = undefined
    if (detailOpen.value) detailOpen.value = false
    detailFocused.value = false
  }

  return {
    sidebarCollapsed, lastSessionId, drawerOpen, shellWidth, shellMode, setShellWidth,
    isWorkspaceExpanded, toggleWorkspace, pinnedSessions, togglePinned, unpin,
    detailOpen, detailView, detailWidth, detailExpanded, detailFocused, lineTabs, activeLineTab, newLine, endingLine,
    setDetailWidth, openDetail, toggleDetail, openLines, toggleLines, openLine, activateLine, closeLineTab, closeDetail,
    viewingSessionId, unread, viewSession, markUnread, splitSessionId, splitAvailable, activePane, openSplit, closeSplit,
    reviewNotes, addReviewNote, removeReviewNote, clearReviewNotes, composerInsert, requestComposerInsert, composerMenu, requestComposerMenu, shortcutsOpen,
    approvalKeyboard,
    requestNewLine, closeNewLine, requestEndLine, closeEndLine, resetSessionTabs,
  }
})
