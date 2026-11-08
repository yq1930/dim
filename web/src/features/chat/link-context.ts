import type { ComputedRef, InjectionKey } from "vue"

/**
 * Where a Session's Markdown links lead: a relative path resolves against `base`
 * (the Session's working directory, or the folder of the file a link is in),
 * and with `filePane` a file of the Session's workspace opens in the detail
 * panel's file view.
 */
export type LinkContext = { sessionId: string; base: string; filePane: boolean }

export const LINK_CONTEXT: InjectionKey<ComputedRef<LinkContext | undefined>> = Symbol("link-context")
