declare global {
  interface Window {
    __TAURI__?: { core: { invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> } }
  }
}

export const canChooseWorkspace = typeof window !== "undefined" && window.__TAURI__ !== undefined

export async function chooseWorkspace(initialDirectory: string): Promise<string | null> {
  if (!window.__TAURI__) throw new Error("The native folder picker is unavailable")
  return window.__TAURI__.core.invoke<string | null>("choose_workspace", { initialDirectory: initialDirectory || null })
}
