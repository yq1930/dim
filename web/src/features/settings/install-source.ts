import { reactive } from "vue"

/**
 * Where a plugin, package or Skill installation comes from: a local directory,
 * or a Git repository at an optional ref; `id` names the installation, else the
 * server derives it from the source.
 */
export type InstallSource = { type: "path" | "git"; location: string; ref: string; id: string }

export function installSource(): InstallSource {
  return reactive({ type: "path", location: "", ref: "", id: "" })
}

/** The `source` object the install and preview methods take. */
export function sourceParams(source: InstallSource): Record<string, unknown> {
  const location = source.location.trim()
  const ref = source.ref.trim()
  return source.type === "path" ? { path: location } : { git: location, ...(ref ? { ref } : {}) }
}

/** `{ id }` when the operator named the installation. */
export function idParams(source: InstallSource): Record<string, unknown> {
  const id = source.id.trim()
  return id ? { id } : {}
}

/** True once anything is typed; the source kind alone is not a change. */
export function sourceEdited(source: InstallSource): boolean {
  return !!source.location || !!source.ref || !!source.id
}

/** Empties the typed fields after an installation, keeping the source kind. */
export function clearSource(source: InstallSource): void {
  source.location = ""
  source.ref = ""
  source.id = ""
}
