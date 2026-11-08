import { ref, watch } from "vue"
import { defineStore } from "pinia"

/** How a transcript shows the agent's work: grouped and folded, with its thinking open, or with everything open. */
export type TranscriptView = "normal" | "thinking" | "verbose"
export const TRANSCRIPT_VIEWS: readonly TranscriptView[] = ["normal", "thinking", "verbose"]

const STORAGE_KEY = "dim-web.preferences.v1"
/** Sessions whose own view is remembered; past this the oldest choice is forgotten. */
const SESSION_VIEWS_MAX = 200

type Persisted = {
  transcriptView: TranscriptView
  sessionViews: Record<string, TranscriptView>
  keepAwake: boolean
  autoArchive: boolean
  openWith?: string
}

function isView(value: unknown): value is TranscriptView {
  return TRANSCRIPT_VIEWS.includes(value as TranscriptView)
}

/** localStorage is outside the app: read it once, keeping only well-formed fields. */
function readPersisted(): Persisted {
  const fallback: Persisted = { transcriptView: "normal", sessionViews: {}, keepAwake: true, autoArchive: false }
  try {
    const value: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "null")
    if (value === null || typeof value !== "object" || Array.isArray(value)) return fallback
    const stored = value as Record<string, unknown>
    const views = stored.sessionViews !== null && typeof stored.sessionViews === "object" && !Array.isArray(stored.sessionViews)
      ? Object.fromEntries(Object.entries(stored.sessionViews).filter((entry): entry is [string, TranscriptView] => isView(entry[1])))
      : {}
    return {
      transcriptView: isView(stored.transcriptView) ? stored.transcriptView : fallback.transcriptView,
      sessionViews: views,
      keepAwake: typeof stored.keepAwake === "boolean" ? stored.keepAwake : fallback.keepAwake,
      autoArchive: typeof stored.autoArchive === "boolean" ? stored.autoArchive : fallback.autoArchive,
      ...(typeof stored.openWith === "string" ? { openWith: stored.openWith } : {}),
    }
  } catch { return fallback }
}

/** Preferences of this browser: transcript views, keeping the computer awake, auto-archive and the app folders open in. */
export const usePreferencesStore = defineStore("preferences", () => {
  const persisted = readPersisted()
  const transcriptView = ref(persisted.transcriptView)
  const sessionViews = ref(persisted.sessionViews)
  const keepAwake = ref(persisted.keepAwake)
  const autoArchive = ref(persisted.autoArchive)
  const openWith = ref(persisted.openWith)
  watch([transcriptView, sessionViews, keepAwake, autoArchive, openWith], () => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify({
        transcriptView: transcriptView.value,
        sessionViews: sessionViews.value,
        keepAwake: keepAwake.value,
        autoArchive: autoArchive.value,
        openWith: openWith.value,
      }))
    } catch { /* best effort: full or disabled storage keeps the choice for this page load */ }
  })

  function viewOf(sessionId: string): TranscriptView {
    return sessionViews.value[sessionId] ?? transcriptView.value
  }

  /** Sets one Session's view; a Session on the default view keeps no entry. */
  function setView(sessionId: string, view: TranscriptView): void {
    const { [sessionId]: _, ...rest } = sessionViews.value
    if (view === transcriptView.value) {
      sessionViews.value = rest
      return
    }
    const kept = Object.entries(rest)
    sessionViews.value = Object.fromEntries([...kept.slice(Math.max(0, kept.length - SESSION_VIEWS_MAX + 1)), [sessionId, view]])
  }

  /** Steps a Session to the next view, as Ctrl+O does in Claude Code. */
  function cycleView(sessionId: string): TranscriptView {
    const next = TRANSCRIPT_VIEWS[(TRANSCRIPT_VIEWS.indexOf(viewOf(sessionId)) + 1) % TRANSCRIPT_VIEWS.length]!
    setView(sessionId, next)
    return next
  }

  /** Makes a Session's view the default, which that Session then follows. */
  function makeDefault(sessionId: string): void {
    transcriptView.value = viewOf(sessionId)
    const { [sessionId]: _, ...rest } = sessionViews.value
    sessionViews.value = rest
  }

  return { transcriptView, sessionViews, keepAwake, autoArchive, openWith, viewOf, setView, cycleView, makeDefault }
})
