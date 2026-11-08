import { ref, watch } from "vue"
import { defineStore } from "pinia"

/** How a transcript shows the agent's work: grouped and folded, with its thinking open, or with everything open. */
export type TranscriptView = "normal" | "thinking" | "verbose"
export const TRANSCRIPT_VIEWS: readonly TranscriptView[] = ["normal", "thinking", "verbose"]

const STORAGE_KEY = "dim-web.preferences.v1"

type Persisted = {
  transcriptView: TranscriptView
  keepAwake: boolean
  autoArchive: boolean
  openWith?: string
}

function isView(value: unknown): value is TranscriptView {
  return TRANSCRIPT_VIEWS.includes(value as TranscriptView)
}

/** localStorage is outside the app: read it once, keeping only well-formed fields. */
function readPersisted(): Persisted {
  const fallback: Persisted = { transcriptView: "normal", keepAwake: true, autoArchive: false }
  try {
    const value: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "null")
    if (value === null || typeof value !== "object" || Array.isArray(value)) return fallback
    const stored = value as Record<string, unknown>
    return {
      transcriptView: isView(stored.transcriptView) ? stored.transcriptView : fallback.transcriptView,
      keepAwake: typeof stored.keepAwake === "boolean" ? stored.keepAwake : fallback.keepAwake,
      autoArchive: typeof stored.autoArchive === "boolean" ? stored.autoArchive : fallback.autoArchive,
      ...(typeof stored.openWith === "string" ? { openWith: stored.openWith } : {}),
    }
  } catch { return fallback }
}

/**
 * Preferences of this browser: the default transcript view (each Session's own
 * is in the panes store), keeping the computer awake, auto-archive and the app
 * folders open in.
 */
export const usePreferencesStore = defineStore("preferences", () => {
  const persisted = readPersisted()
  const transcriptView = ref(persisted.transcriptView)
  const keepAwake = ref(persisted.keepAwake)
  const autoArchive = ref(persisted.autoArchive)
  const openWith = ref(persisted.openWith)
  watch([transcriptView, keepAwake, autoArchive, openWith], () => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify({
        transcriptView: transcriptView.value,
        keepAwake: keepAwake.value,
        autoArchive: autoArchive.value,
        openWith: openWith.value,
      }))
    } catch { /* best effort: full or disabled storage keeps the choice for this page load */ }
  })

  return { transcriptView, keepAwake, autoArchive, openWith }
})
