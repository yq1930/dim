declare global {
  interface Window {
    __TAURI__?: { core: { invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> } }
  }
}

export const canChooseWorkspace = typeof window !== "undefined" && window.__TAURI__ !== undefined

/**
 * The desktop window on macOS draws no title bar: the page runs under the window
 * controls, leaves them room at its top-left and drags the window from its top bars.
 */
export const overlayTitleBar = canChooseWorkspace && navigator.userAgent.includes("Mac")

export async function chooseWorkspace(initialDirectory: string): Promise<string | null> {
  if (!window.__TAURI__) throw new Error("The native folder picker is unavailable")
  return window.__TAURI__.core.invoke<string | null>("choose_workspace", { initialDirectory: initialDirectory || null })
}

/** Alerts the user through the system. The desktop shell drops it while its window is in front. */
export function notifyDesktop(title: string, body: string): void {
  void window.__TAURI__?.core.invoke("notify_attention", { title, body }).catch(() => undefined)
}
