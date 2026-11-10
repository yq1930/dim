import { computed, ref, watch, type Ref } from "vue"

/** A row of the `/` menu: a built-in command or a Skill. */
export type MenuEntry = { category: string; name: string; description?: string }

/** A built-in command, listed only when `available`. */
export type CommandEntry = { category: string; name: string; available: boolean }

/** The commands whose words follow them; choosing one in the menu fills `/name ` for them. */
export const ARGUMENT_COMMANDS: readonly string[] = ["branch", "goal", "effort", "review", "rename"]

/** `/name words` when `message` is that command, else undefined. */
export function commandWords(message: string, name: string): string | undefined {
  const match = new RegExp(`^/${name}(?:\\s+([\\s\\S]*))?$`, "i").exec(message)
  return match === null ? undefined : match[1]?.trim() ?? ""
}

/**
 * The `/` menu of a composer: open for a leading `/` and a command-name prefix,
 * listing the available built-in commands and then the Skills `listSkills`
 * returns, filtered by the prefix. It stays closed while an IME composes and
 * after Escape marks the draft literal (`dismissed`). Arrow keys move the
 * highlighted row, which a new prefix resets to the first.
 */
export function useCommandMenu(options: {
  text: Ref<string>
  focused: Ref<boolean>
  composing: Ref<boolean>
  dismissed: Ref<boolean>
  commands: () => readonly CommandEntry[]
  listSkills: () => (() => Promise<Array<{ name: string; description: string }>>) | undefined
}) {
  const { text, focused, composing, dismissed } = options
  const prefix = computed(() => /^\/[\w-]*$/.test(text.value) ? text.value.slice(1).toLowerCase() : undefined)
  const skills = ref<Array<{ name: string; description: string }>>([])
  watch(() => prefix.value !== undefined, async (typing) => {
    const list = options.listSkills()
    if (!typing || list === undefined) return
    try { skills.value = await list() } catch { skills.value = [] }
  })
  const matches = computed<MenuEntry[]>(() => {
    const typed = prefix.value
    if (typed === undefined) return []
    const commands = options.commands().filter((command) => command.available)
    const skillEntries = skills.value.map((skill) => ({ category: "skill", name: skill.name, description: skill.description }))
    return [...commands, ...skillEntries].filter((entry) => entry.name.toLowerCase().startsWith(typed))
  })
  const commandMode = computed(() => !composing.value && !dismissed.value && matches.value.length > 0)
  const open = computed(() => focused.value && commandMode.value)
  const active = ref(0)

  watch(text, () => {
    if (!text.value.startsWith("/")) dismissed.value = false
  }, { flush: "sync" })
  watch(matches, () => { active.value = 0 }, { flush: "sync" })

  return {
    matches, commandMode, open, active,
    move: (step: 1 | -1) => { active.value = (active.value + step + matches.value.length) % matches.value.length },
  }
}
