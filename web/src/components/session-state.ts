import { useChatStore } from "../stores/chat"
import { useUiStore } from "../stores/ui"
import type { SessionSummary } from "../rpc/protocol"

export type SessionState = "waiting" | "running" | "unread" | "idle"

/**
 * How a Session reads in the sidebar. It waits on the operator while a line
 * holds an approval, a question or a plan review; a run that ended while
 * another page was shown leaves it unread.
 */
export function useSessionState(): (session: SessionSummary) => SessionState {
  const chat = useChatStore()
  const ui = useUiStore()
  return (session) => {
    const view = chat.views[session.sessionId]
    if (view !== undefined && (view.approvals.length > 0 || view.questions.length > 0 || view.reviews.length > 0)) return "waiting"
    if (session.running) return "running"
    return ui.unread.includes(session.sessionId) ? "unread" : "idle"
  }
}
