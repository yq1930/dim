import { getCurrentInstance, reactive, type App } from "vue"
import type { FileMention } from "../../mentions"
import type { DraftFile, ImageBlock } from "../../rpc/protocol"

export type ComposerDraft = {
  text: string
  mentions: FileMention[]
  images: Array<{ id: number; url: string; block: ImageBlock }>
  pastes: Array<{ id: number; text: string }>
  files: Array<DraftFile & { id: number }>
  nextId: number
  reading: number
  sending: boolean
  error: string
}
const applications = new WeakMap<App, Map<string, ComposerDraft>>()
export function createComposerDraft(): ComposerDraft {
  return reactive({ text: "", mentions: [], images: [], pastes: [], files: [], nextId: 0, reading: 0, sending: false, error: "" })
}
export function useComposerDrafts() {
  const app = getCurrentInstance()!.appContext.app
  let drafts = applications.get(app)
  if (!drafts) { drafts = new Map(); applications.set(app, drafts) }
  const records = drafts
  return {
    get(key: string): ComposerDraft {
      let draft = records.get(key)
      if (!draft) { draft = createComposerDraft(); records.set(key, draft) }
      return draft
    },
    discardSession(sessionId: string): void {
      for (const key of records.keys()) if (key.startsWith(`session:${sessionId}:`)) records.delete(key)
    },
  }
}
