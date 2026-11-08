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

/**
 * Opens an absolute path with its default application, or shows it in Finder
 * (`reveal`, and whatever the shell will not open).
 */
export async function openWorkspacePath(path: string, reveal: boolean): Promise<void> {
  if (!window.__TAURI__) throw new Error("Opening files needs the desktop app")
  await window.__TAURI__.core.invoke("open_workspace_path", { path, reveal })
}

/** Alerts the user through the system. The desktop shell drops it while its window is in front. */
export function notifyDesktop(title: string, body: string): void {
  void window.__TAURI__?.core.invoke("notify_attention", { title, body }).catch(() => undefined)
}

/** An application the desktop app opens a folder in: an editor, a terminal or Finder. */
export type DesktopApp = { id: string; name: string; kind: "editor" | "terminal" | "finder"; icon?: string }

let installed: Promise<DesktopApp[]> | undefined

/** The editors, terminals and Finder installed, asked for once per page load; none outside the desktop app. */
export function installedApps(): Promise<DesktopApp[]> {
  const tauri = window.__TAURI__
  if (tauri === undefined) return Promise.resolve([])
  installed ??= tauri.core.invoke<DesktopApp[]>("installed_apps").catch(() => {
    installed = undefined
    return []
  })
  return installed
}

/** Opens an absolute path in one of the installed applications. */
export async function openInApp(id: string, path: string): Promise<void> {
  if (!window.__TAURI__) throw new Error("Opening a folder in another app needs the desktop app")
  await window.__TAURI__.core.invoke("open_in_app", { id, path })
}

/** Keeps the computer from idle-sleeping while `active`; a browser has no such control. */
export function keepAwake(active: boolean): void {
  void window.__TAURI__?.core.invoke("keep_awake", { active }).catch(() => undefined)
}

/** Lets the desktop window's preview frame show a dev server on this computer; a browser needs nothing. */
export async function showPreview(url: string): Promise<void> {
  await window.__TAURI__?.core.invoke("show_preview", { url })
}
