import { ref } from "vue"
import { useRoute, useRouter } from "vue-router"
import { useI18n } from "vue-i18n"
import { copyText } from "../../clipboard"
import type { TranslationKey } from "../../i18n"
import type { SessionSummary } from "../../rpc/protocol"
import { useSessionsStore } from "../../stores/sessions"
import { useToastStore } from "../../stores/toast"
import { useUiStore } from "../../stores/ui"
import { useComposerDrafts } from "../session/composer/composer-drafts"

/** What a Session's menus do beyond navigation, for the sidebar's rows and the session header's title. */
export function useSessionActions() {
  const { t } = useI18n()
  const route = useRoute()
  const router = useRouter()
  const sessions = useSessionsStore()
  const toast = useToastStore()
  const ui = useUiStore()
  const drafts = useComposerDrafts()
  const busy = ref(false)

  async function copy(text: string, done: TranslationKey): Promise<void> {
    try {
      await copyText(text)
      toast.push({ tone: "success", title: t(done) })
    } catch {
      toast.push({ tone: "danger", title: t("chat.copyFailed") })
    }
  }

  /** Archives or deletes `session` (a deletion is confirmed by the caller), leaving its page for a new Session in its workspace. */
  async function dispose(kind: "archive" | "delete", session: SessionSummary): Promise<void> {
    if (busy.value) return
    busy.value = true
    try {
      if (kind === "archive") await sessions.setArchived(session.sessionId, true)
      else {
        await sessions.remove(session.sessionId)
        drafts.discardSession(session.sessionId)
        ui.unpin(session.sessionId)
      }
      if (ui.lastSessionId === session.sessionId) ui.lastSessionId = undefined
      if (route.params.sessionId === session.sessionId) await router.replace({ name: "chat-new", query: { cwd: session.cwd } })
    } catch (cause) {
      toast.push({ tone: "danger", title: t("sidebar.actionFailed"), description: cause instanceof Error ? cause.message : String(cause) })
    } finally {
      busy.value = false
    }
  }

  return { busy, copy, dispose }
}
