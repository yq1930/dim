import type { ComputedRef, InjectionKey } from "vue"

/** The directory a Session's relative Markdown links resolve against: its working directory. */
export const LINK_BASE: InjectionKey<ComputedRef<string | undefined>> = Symbol("link-base")
