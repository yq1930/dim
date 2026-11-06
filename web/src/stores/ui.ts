import { computed, ref, watch } from "vue"
import { defineStore } from "pinia"
import { approvalKeyboardBranch } from "../features/lines/line-state"
import { useAppearanceStore } from "./appearance"

export type ShellMode = "compact" | "regular" | "wide"

/** What the right detail panel shows: the Session's lines, or the files it changed. */
export type DetailView = "lines" | "changes"

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

/** Layout state remembered in this browser. */
type Persisted = {
  sidebarCollapsed: boolean
  collapsedWorkspaces: string[]
  lastSessionId: string | undefined
  detailOpen: boolean
  detailView: DetailView
  detailWidth: number
}

/** localStorage is outside the app: read it once, keeping only well-formed fields. */
function readPersisted(): Persisted {
  const fallback: Persisted = { sidebarCollapsed: false, collapsedWorkspaces: [], lastSessionId: undefined, detailOpen: false, detailView: "lines", detailWidth: 420 }
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
      detailView: stored.detailView === "changes" ? "changes" : "lines",
      detailWidth: width,
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

  watch([sidebarCollapsed, collapsedWorkspaces, lastSessionId, detailOpen, detailView, detailWidth], () => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify({
        sidebarCollapsed: sidebarCollapsed.value,
        collapsedWorkspaces: collapsedWorkspaces.value,
        lastSessionId: lastSessionId.value,
        detailOpen: detailOpen.value,
        detailView: detailView.value,
        detailWidth: detailWidth.value,
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
    if (sessionId !== viewingSessionId.value && !unread.value.includes(sessionId)) unread.value = [...unread.value, sessionId]
  }

  function openChanges(): void {
    detailView.value = "changes"
    detailOpen.value = true
    detailFocused.value = true
  }

  function toggleChanges(): void {
    if (detailOpen.value && detailView.value === "changes") {
      detailOpen.value = false
      detailFocused.value = false
    } else openChanges()
  }

  function openLines(sessionId: string): void {
    detailView.value = "lines"
    detailOpen.value = true
    detailFocused.value = true
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
    detailView.value = "lines"
    detailOpen.value = true
    detailFocused.value = true
  }

  function activateLine(sessionId: string, branchId: string): void {
    if (!(lineTabs.value[sessionId] ?? []).includes(branchId)) {
      openLine(sessionId, branchId)
      return
    }
    activeLineTab.value = { ...activeLineTab.value, [sessionId]: branchId }
    detailView.value = "lines"
    detailOpen.value = true
    detailFocused.value = true
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
    isWorkspaceExpanded, toggleWorkspace,
    detailOpen, detailView, detailWidth, detailExpanded, detailFocused, lineTabs, activeLineTab, newLine, endingLine,
    setDetailWidth, openChanges, toggleChanges, openLines, toggleLines, openLine, activateLine, closeLineTab, closeDetail,
    viewingSessionId, unread, viewSession, markUnread,
    approvalKeyboard,
    requestNewLine, closeNewLine, requestEndLine, closeEndLine, resetSessionTabs,
  }
})
