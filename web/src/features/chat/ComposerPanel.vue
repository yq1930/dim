<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, useId, watch } from "vue"
import { useRoute, useRouter } from "vue-router"
import { useI18n } from "vue-i18n"
import { ArrowUp, FileSearch, FileText, Paperclip, Square, X } from "lucide-vue-next"
import type { ContextUsage, DraftAttachments, FileCandidate, ImageBlock, ModelId, PermissionPreset, PermissionPolicy } from "../../rpc/protocol"
import { formatTokens } from "./usage-format"
import { linkMentions, mentionTrigger, type FileMention, type MentionTrigger } from "../../mentions"
import { useModelsStore } from "../../stores/models"
import { useSessionsStore } from "../../stores/sessions"
import ComposerModeMenu, { type WorkMode } from "./ComposerModeMenu.vue"
import ComposerModelMenu from "./ComposerModelMenu.vue"
import WorkspaceFileDialog from "./WorkspaceFileDialog.vue"
import { createComposerDraft, useComposerDrafts, type ComposerDraft } from "./composer-drafts"
import { quoteInto } from "./quote"

/** The Session commands the menu lists, each only when given. Errors are reported by the callee. */
type SessionCommands = {
  compact?: () => Promise<void>
  usage?: () => Promise<void>
  fork?: () => Promise<void>
  /** Receives the words after `/goal`; empty shows the goal. */
  goal?: (words: string) => Promise<void>
  /** Receives the words after `/effort`; empty lists the choices. */
  effort?: (words: string) => Promise<void>
}

/**
 * The message composer: a growing text field, the model menu, and one button
 * that sends, or stops the run while one is in flight. Enter sends,
 * Shift+Enter breaks the line.
 */
const props = withDefaults(defineProps<{
  running: boolean
  stopping?: boolean
  model: ModelId | undefined
  draftKey?: string
  /** False when this connection may not change the model; the menu then only names it. */
  modelSelectable: boolean
  /** Sends the text and the draft's attachments; resolves once the server accepted it. */
  send: (text: string, attachments: DraftAttachments) => Promise<void>
  /** False where a draft cannot carry attachments. */
  attachments?: boolean
  /** The Skills a `/` command can reference; the menu lists only built-in commands without it. */
  listSkills?: () => Promise<Array<{ name: string; description: string }>>
  /** The latest context window reading, shown beside the model. */
  contextUsage?: ContextUsage
  /** Plan mode of the line, when it can be switched here; the mode menu offers it beside the presets. */
  planMode?: { active: boolean; toggle: () => void | Promise<void> }
  /** True when the Session stores long pastes through `attachment.put`. */
  storesPastes?: boolean
  /** Finds the files an `@` mention offers for a query; absent where the Session serves no file search. */
  searchFiles?: (query: string) => Promise<FileCandidate[]>
  /** The Session policy and its independent controls; absent where permission methods are unavailable. */
  permission?: { policy: PermissionPolicy | undefined; select: (preset: PermissionPreset) => Promise<void> }
  stop?: () => Promise<void>
  selectModel: (model: ModelId) => Promise<void> | void
  reasoningEffort?: string
  selectEffort?: (effort: string | undefined) => Promise<void> | void
  /** The narrower line panel: no model menu, less vertical chrome. */
  variant?: "main" | "panel"
  placeholder?: string
  createBranch?: (prompt: string) => Promise<void>
  openLines?: () => void
  /** Session commands the menu lists, each only when given. */
  sessionCommands?: SessionCommands
}>(), { attachments: true })
const { t } = useI18n()
const route = useRoute()
const router = useRouter()

const models = useModelsStore()
const sessions = useSessionsStore()
const field = ref<HTMLTextAreaElement>()
const upload = ref<HTMLInputElement>()
const fileDialog = ref<InstanceType<typeof WorkspaceFileDialog>>()
const drafts = useComposerDrafts()
const localDraft = createComposerDraft()
const draft = computed(() => props.draftKey === undefined ? localDraft : drafts.get(props.draftKey))
const modelMenu = ref<InstanceType<typeof ComposerModelMenu>>()
const modeMenu = ref<InstanceType<typeof ComposerModeMenu>>()
const menuId = useId()
const text = computed({ get: () => draft.value.text, set: (value) => { draft.value.text = value } })
const sending = computed({ get: () => draft.value.sending, set: (value) => { draft.value.sending = value } })
const changingModel = ref(false)
const changingEffort = ref(false)
const changingMode = ref(false)
/** The presets from the most asking to the least, then plan mode and read-only. */
const workModes = computed<WorkMode[]>(() => [
  ...(props.permission === undefined ? [] : ["ask", "workspace-write", "full-access"] as const),
  ...(props.planMode === undefined ? [] : ["plan"] as const),
  ...(props.permission === undefined ? [] : ["read-only"] as const),
])
const workMode = computed<WorkMode | undefined>(() => props.planMode?.active ? "plan" : props.permission?.policy?.preset)
const error = computed({ get: () => draft.value.error, set: (value) => { draft.value.error = value } })
const composing = ref(false)
const focused = ref(false)
const dismissed = ref(false)
/** The files inserted into the draft; each sends as a Markdown link to its path. */
const fileMentions = computed({ get: () => draft.value.mentions, set: (value: FileMention[]) => { draft.value.mentions = value } })
const mention = ref<MentionTrigger | undefined>()
const mentionDismissed = ref<string | undefined>()
const candidates = ref<FileCandidate[]>([])
const candidateActive = ref(0)
let searchSeq = 0
let searchTimer: ReturnType<typeof setTimeout> | undefined
const mentionKey = computed(() => mention.value === undefined ? undefined : `${mention.value.start}:${mention.value.query}`)
const mentionOpen = computed(() => focused.value && props.searchFiles !== undefined && mention.value !== undefined && mentionKey.value !== mentionDismissed.value)
const mentionShown = computed(() => mentionOpen.value && candidates.value.length > 0)
const mentionMenuId = useId()
const activeIndex = ref(0)
/** The limits of `crates/dim-cli/src/attachments.rs`: a paste longer than this is stored as a Session attachment. */
const INLINE_PASTE = 16 * 1024
const MAX_PASTE = 10 * 1024 * 1024
const MAX_ATTACHMENTS = 20
const images = computed({ get: () => draft.value.images, set: (value) => { draft.value.images = value } })
const pastes = computed({ get: () => draft.value.pastes, set: (value) => { draft.value.pastes = value } })
const files = computed({ get: () => draft.value.files, set: (value) => { draft.value.files = value } })
const IMAGE_TYPES = new Set(["image/png", "image/jpeg", "image/webp", "image/gif"])
const FILE_ACCEPT = "image/png,image/jpeg,image/webp,image/gif,application/pdf,.pdf,text/*,application/json,application/javascript,application/xml,.ts,.tsx,.jsx,.vue,.rs,.go,.py,.php,.sql,.md,.yaml,.yml,.toml,.ini,.sh,.css,.scss,.jsonc,.log,.diff,.patch,.svg"
watch(() => props.draftKey, () => { mention.value = undefined; candidates.value = []; fileDialog.value?.close(); void nextTick(resize) })

const availableCommands = computed(() => ([
  { category: "model", name: "models", available: props.modelSelectable && models.available && !changingModel.value },
  { category: "session", name: "branch", available: props.createBranch !== undefined },
  { category: "session", name: "compact", available: props.sessionCommands?.compact !== undefined },
  { category: "session", name: "usage", available: props.sessionCommands?.usage !== undefined },
  { category: "session", name: "fork", available: props.sessionCommands?.fork !== undefined },
  { category: "session", name: "goal", available: props.sessionCommands?.goal !== undefined },
  { category: "model", name: "effort", available: props.sessionCommands?.effort !== undefined },
  { category: "model", name: "permissions", available: props.permission !== undefined && props.variant !== "panel" },
  { category: "session", name: "lines", available: props.openLines !== undefined },
  { category: "session", name: "plan", available: props.planMode !== undefined },
  { category: "session", name: "new", available: true },
] as const).filter((command) => command.available))
const commandPrefix = computed(() => /^\/[\w-]*$/.test(text.value) ? text.value.slice(1).toLowerCase() : undefined)
const skills = ref<Array<{ name: string; description: string }>>([])
watch(() => commandPrefix.value !== undefined, async (typing) => {
  if (!typing || props.listSkills === undefined) return
  try { skills.value = await props.listSkills() } catch { skills.value = [] }
})
type MenuEntry = { category: string; name: string; description?: string }
const matches = computed<MenuEntry[]>(() => {
  const prefix = commandPrefix.value
  if (prefix === undefined) return []
  const skillEntries = skills.value.map((skill) => ({ category: "skill", name: skill.name, description: skill.description }))
  return [...availableCommands.value, ...skillEntries].filter((entry) => entry.name.toLowerCase().startsWith(prefix))
})
const contextLabel = computed(() => {
  const reading = props.contextUsage
  return reading === undefined || reading.contextWindow <= 0 ? undefined : `${formatTokens(reading.estimatedInputTokens)} / ${formatTokens(reading.contextWindow)}`
})
const contextNearLimit = computed(() => {
  const reading = props.contextUsage
  return reading !== undefined && reading.thresholdTokens > 0 && reading.estimatedInputTokens >= reading.thresholdTokens * 0.9
})
const commandMode = computed(() => !composing.value && !dismissed.value && matches.value.length > 0)
const menuOpen = computed(() => focused.value && commandMode.value)

watch(text, () => {
  if (!text.value.startsWith("/")) dismissed.value = false
}, { flush: "sync" })
watch(matches, () => { activeIndex.value = 0 }, { flush: "sync" })

const showStop = computed(() => props.running && props.stop !== undefined && text.value.trim() === "" && images.value.length === 0 && pastes.value.length === 0 && files.value.length === 0)
const canSend = computed(() => (text.value.trim() !== "" || images.value.length > 0 || pastes.value.length > 0 || files.value.length > 0) && !sending.value && !draft.value.reading && !changingModel.value && !changingEffort.value && !changingMode.value)

function resize(): void {
  const element = field.value
  if (!element) return
  element.style.height = "auto"
  element.style.height = `${Math.min(element.scrollHeight, 300)}px`
}

function readImage(file: File): Promise<{ url: string; block: ImageBlock }> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => {
      const url = String(reader.result)
      resolve({ url, block: { type: "image", source: { type: "base64", mediaType: file.type, data: url.slice(url.indexOf(",") + 1) } } })
    }
    reader.onerror = () => reject(reader.error ?? new Error(t("composer.imageReadFailed")))
    reader.readAsDataURL(file)
  })
}

async function readPdf(file: File): Promise<string> {
  const head = new TextDecoder("latin1").decode(await file.slice(0, 5).arrayBuffer())
  if (head !== "%PDF-") throw new Error(t("composer.unsupportedFile", { name: file.name }))
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => {
      const url = String(reader.result)
      resolve(url.slice(url.indexOf(",") + 1))
    }
    reader.onerror = () => reject(reader.error ?? new Error(t("composer.unsupportedFile", { name: file.name })))
    reader.readAsDataURL(file)
  })
}

function attachmentCount(target: ComposerDraft): number { return target.images.length + target.pastes.length + target.files.length }

async function addFiles(selected: File[]): Promise<void> {
  const target = draft.value
  const textAllowed = props.storesPastes
  target.reading++
  target.error = ""
  try {
    for (const file of selected) {
      if (attachmentCount(target) >= MAX_ATTACHMENTS) { target.error = t("composer.attachmentLimit"); break }
      if (file.size > MAX_PASTE) { target.error = t("composer.fileTooLarge", { name: file.name }); continue }
      try {
        if (IMAGE_TYPES.has(file.type)) {
          const image = await readImage(file)
          if (target.images.reduce((size, item) => size + item.block.source.data.length, 0) + image.block.source.data.length > MAX_PASTE) { target.error = t("composer.imagesTooLarge"); continue }
          if (attachmentCount(target) < MAX_ATTACHMENTS) target.images = [...target.images, { id: ++target.nextId, ...image }]
        } else if (textAllowed && (file.type === "application/pdf" || /\.pdf$/i.test(file.name))) {
          const pdfBase64 = await readPdf(file)
          if (attachmentCount(target) < MAX_ATTACHMENTS) target.files = [...target.files, { id: ++target.nextId, name: file.name, text: "", pdfBase64 }]
        } else {
          if (!textAllowed || /\.(pdf|docx?|xlsx?|pptx?|zip|gz|7z|rar|exe|dmg|mp[34]|mov|wav|bin)$/i.test(file.name) || (file.type.startsWith("image/") && file.type !== "image/svg+xml")) throw new Error(t("composer.unsupportedFile", { name: file.name }))
          const bytes = await file.arrayBuffer()
          const value = new TextDecoder("utf-8", { fatal: true }).decode(bytes)
          if (value.includes("\0")) throw new Error(t("composer.unsupportedFile", { name: file.name }))
          if (new TextEncoder().encode(JSON.stringify(value)).length > 12 * 1024 * 1024) { target.error = t("composer.fileTooLarge", { name: file.name }); continue }
          if (attachmentCount(target) < MAX_ATTACHMENTS) target.files = [...target.files, { id: ++target.nextId, name: file.name, text: value }]
        }
      } catch (cause) { target.error = cause instanceof TypeError ? t("composer.unsupportedFile", { name: file.name }) : cause instanceof Error ? cause.message : t("composer.imageReadFailed") }
    }
  } finally { target.reading-- }
}

function addPaste(value: string): void {
  if (new TextEncoder().encode(value).length > MAX_PASTE) {
    error.value = t("composer.pasteTooLarge")
    return
  }
  if (new TextEncoder().encode(JSON.stringify(value)).length > 12 * 1024 * 1024) { error.value = t("composer.messageTooLarge"); return }
  if (attachmentCount(draft.value) >= MAX_ATTACHMENTS) { error.value = t("composer.attachmentLimit"); return }
  pastes.value = [...pastes.value, { id: ++draft.value.nextId, text: value }]
}

function onPaste(event: ClipboardEvent): void {
  if (!props.attachments) return
  const pictures = [...(event.clipboardData?.files ?? [])].filter((file) => file.type.startsWith("image/"))
  if (pictures.length > 0) {
    event.preventDefault()
    void addFiles(pictures)
    return
  }
  const value = event.clipboardData?.getData("text/plain") ?? ""
  if (props.storesPastes && new TextEncoder().encode(value).length > INLINE_PASTE) {
    event.preventDefault()
    addPaste(value)
  }
}

function onDrop(event: DragEvent): void {
  event.preventDefault()
  if (!props.attachments) return
  void addFiles([...(event.dataTransfer?.files ?? [])])
}

function chooseFiles(event: Event): void {
  const input = event.target as HTMLInputElement
  void addFiles([...(input.files ?? [])])
  input.value = ""
}

function removeImage(id: number): void {
  images.value = images.value.filter((image) => image.id !== id)
}

function removePaste(id: number): void {
  pastes.value = pastes.value.filter((paste) => paste.id !== id)
}

/** The `@` mention the caret is in, read from the field's own value (the field is the source of truth for the caret). */
function updateMention(): void {
  const element = field.value
  mention.value = element === undefined || props.searchFiles === undefined ? undefined : mentionTrigger(element.value, element.selectionStart)
}

watch(() => mention.value?.query, (query) => {
  const seq = ++searchSeq
  clearTimeout(searchTimer)
  candidateActive.value = 0
  const search = props.searchFiles
  if (query === undefined || search === undefined) {
    candidates.value = []
    return
  }
  const target = draft.value
  searchTimer = setTimeout(() => {
    search(query).then((found) => {
      if (seq === searchSeq) candidates.value = found
    }, (cause: unknown) => {
      if (seq !== searchSeq) return
      candidates.value = []
      target.error = cause instanceof Error ? cause.message : String(cause)
    })
  }, 80)
})

/** Inserts the chosen file as `@path` (a space after it) or steps into a directory as `@dir/`, as the TUI picker does. */
function acceptCandidate(candidate: FileCandidate): void {
  const element = field.value
  const trigger = mention.value
  if (element === undefined || trigger === undefined) return
  const cursor = element.selectionStart
  const label = `@${candidate.relativePath}${candidate.dir ? "/" : ""}`
  const separator = candidate.dir ? "" : " "
  text.value = `${text.value.slice(0, trigger.start)}${label}${separator}${text.value.slice(cursor)}`
  if (!candidate.dir) fileMentions.value = [...fileMentions.value, { label: `@${candidate.relativePath}`, path: candidate.absolutePath }]
  const caret = trigger.start + label.length + separator.length
  void nextTick(() => {
    element.setSelectionRange(caret, caret)
    resize()
    updateMention()
  })
}

function insertFile(candidate: FileCandidate): void {
  const element = field.value
  if (!element) return
  const start = element.selectionStart
  const end = element.selectionEnd
  const label = `@${candidate.relativePath}`
  const prefix = start > 0 && !/\s/.test(text.value[start - 1]!) ? " " : ""
  text.value = `${text.value.slice(0, start)}${prefix}${label} ${text.value.slice(end)}`
  fileMentions.value = [...fileMentions.value, { label, path: candidate.absolutePath }]
  void nextTick(() => { element.focus(); element.setSelectionRange(start + prefix.length + label.length + 1, start + prefix.length + label.length + 1); resize() })
}

async function submit(): Promise<void> {
  if (!canSend.value || composing.value) return
  const target = draft.value
  if (commandMode.value) {
    const entry = matches.value[activeIndex.value]
    if (entry !== undefined) await runMenuEntry(entry)
    return
  }
  const message = text.value
  const branchCommand = !dismissed.value && /^\/branch(?:\s+([\s\S]*))?$/i.exec(message)
  if (branchCommand && !branchCommand[1]?.trim()) {
    await runCommand("branch")
    return
  }
  const goal = props.sessionCommands?.goal
  const goalCommand = !dismissed.value && goal !== undefined ? /^\/goal(?:\s+([\s\S]*))?$/i.exec(message) : null
  if (goal && goalCommand) {
    text.value = ""
    await nextTick()
    resize()
    await goal(goalCommand[1]?.trim() ?? "")
    return
  }
  const effort = props.sessionCommands?.effort
  const effortCommand = !dismissed.value && effort !== undefined ? /^\/effort(?:\s+([\s\S]*))?$/i.exec(message) : null
  if (effort && effortCommand) {
    text.value = ""
    await nextTick()
    resize()
    await effort(effortCommand[1]?.trim() ?? "")
    return
  }
  sending.value = true
  error.value = ""
  try {
    if (branchCommand) {
      if (!props.createBranch) throw new Error(t("composer.branchUnavailable"))
      await props.createBranch(branchCommand[1]!.trim())
    } else {
      const sentMentions = target.mentions
      const sentImages = target.images
      const sentPastes = target.pastes
      const sentFiles = target.files
      if (new TextEncoder().encode(JSON.stringify({ text: linkMentions(message, sentMentions), images: sentImages.map((image) => image.block) })).length > 14 * 1024 * 1024) throw new Error(t("composer.messageTooLarge"))
      await props.send(linkMentions(message, sentMentions), { images: sentImages.map((image) => image.block), pastes: sentPastes.map((paste) => paste.text), ...(sentFiles.length ? { files: sentFiles.map(({ id: _id, ...file }) => file) } : {}) })
      target.mentions = target.mentions.filter((mention) => !sentMentions.includes(mention))
      target.images = target.images.filter((image) => !sentImages.includes(image))
      target.pastes = target.pastes.filter((paste) => !sentPastes.includes(paste))
      target.files = target.files.filter((file) => !sentFiles.includes(file))
    }
    if (target.text === message) target.text = ""
    await nextTick()
    resize()
  } catch (cause) {
    target.error = cause instanceof Error ? cause.message : String(cause)
  } finally {
    target.sending = false
  }
}

async function stop(): Promise<void> {
  error.value = ""
  try {
    await props.stop?.()
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause)
  }
}

async function onModel(model: ModelId): Promise<void> {
  if (changingModel.value || changingEffort.value) return
  const target = draft.value
  changingModel.value = true
  target.error = ""
  try {
    await props.selectModel(model)
  } catch (cause) {
    target.error = cause instanceof Error ? cause.message : String(cause)
  } finally {
    changingModel.value = false
  }
}

async function onEffort(effort: string | undefined): Promise<void> {
  if (!props.selectEffort || changingEffort.value || changingModel.value) return
  const target = draft.value
  changingEffort.value = true
  target.error = ""
  try {
    await props.selectEffort(effort)
  } catch (cause) {
    target.error = cause instanceof Error ? cause.message : String(cause)
  } finally {
    changingEffort.value = false
  }
}

async function togglePlan(): Promise<void> {
  try {
    await props.planMode?.toggle()
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause)
  }
}

/**
 * Plan mode turns on over the current preset; any other mode first ends plan
 * mode, which restores the preset it replaced, then sets the chosen preset.
 */
async function onMode(mode: WorkMode): Promise<void> {
  if (changingMode.value) return
  changingMode.value = true
  error.value = ""
  try {
    if (mode === "plan") {
      if (props.planMode?.active === false) await props.planMode.toggle()
      return
    }
    const planning = props.planMode?.active === true
    if (planning) await props.planMode?.toggle()
    if (props.permission !== undefined && (planning || props.permission.policy?.preset !== mode)) await props.permission.select(mode)
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause)
  } finally {
    changingMode.value = false
  }
}

async function runMenuEntry(entry: MenuEntry): Promise<void> {
  if (entry.category !== "skill") return runCommand(entry.name)
  if (composing.value) return
  error.value = ""
  text.value = `$skill: ${entry.name} `
  await nextTick()
  resize()
  field.value?.focus()
}

async function runCommand(name: string): Promise<void> {
  if (composing.value) return
  error.value = ""
  const argument = name === "branch" || name === "goal" || name === "effort"
  text.value = argument ? `/${name} ` : ""
  await nextTick()
  resize()
  if (argument) field.value?.focus()
  else if (name === "models") modelMenu.value?.open()
  else if (name === "plan") await togglePlan()
  else if (name === "lines") props.openLines?.()
  else if (name === "new") {
    try {
      const cwd = typeof route.params.sessionId === "string" ? sessions.byId.get(route.params.sessionId)?.cwd : undefined
      await router.push({ name: "chat-new", ...(cwd === undefined ? {} : { query: { cwd } }) })
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : String(cause)
    }
  } else if (name === "permissions") modeMenu.value?.open()
  else if (name === "compact" || name === "usage" || name === "fork") await props.sessionCommands?.[name]?.()
}

function onKeydown(event: KeyboardEvent): void {
  if (event.isComposing || composing.value || event.keyCode === 229) return
  if (mentionOpen.value && candidates.value.length > 0) {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault()
      candidateActive.value = (candidateActive.value + (event.key === "ArrowDown" ? 1 : -1) + candidates.value.length) % candidates.value.length
      return
    }
    if ((event.key === "Enter" && !event.shiftKey && !event.altKey) || event.key === "Tab") {
      event.preventDefault()
      acceptCandidate(candidates.value[candidateActive.value]!)
      return
    }
  }
  if (mentionOpen.value && event.key === "Escape") {
    event.preventDefault()
    event.stopPropagation()
    mentionDismissed.value = mentionKey.value
    return
  }
  if (event.key === "Escape" && text.value.startsWith("/") && !dismissed.value) {
    event.preventDefault()
    event.stopPropagation()
    dismissed.value = true
    return
  }
  if (menuOpen.value && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
    event.preventDefault()
    activeIndex.value = (activeIndex.value + (event.key === "ArrowDown" ? 1 : -1) + matches.value.length) % matches.value.length
    return
  }
  if (event.key === "Enter" && !event.shiftKey && !event.altKey) {
    event.preventDefault()
    void submit()
  }
}

/** Quotes `message` at the end of the draft (see quote.ts). */
async function quote(message: string): Promise<void> {
  text.value = quoteInto(text.value, message)
  await nextTick()
  resize()
  const element = field.value
  element?.focus()
  element?.setSelectionRange(text.value.length, text.value.length)
}

onMounted(() => { resize(); field.value?.focus() })
onBeforeUnmount(() => { ++searchSeq; clearTimeout(searchTimer) })
defineExpose({ focus: () => field.value?.focus(), quote })
</script>

<template>
  <section class="composer-panel" :data-variant="variant ?? 'main'" :aria-label="t('chat.composer')">
    <p v-if="error" class="inline-error" role="alert">{{ error }}</p>
    <div class="composer-surface" @click.self="field?.focus()" @dragover.prevent @drop="onDrop">
      <div v-if="images.length > 0 || pastes.length > 0 || files.length > 0" class="draft-attachments">
        <span v-for="image in images" :key="image.id" class="draft-chip">
          <img :src="image.url" :alt="t('composer.imageAttachment')" class="draft-thumb">
          <button type="button" class="ui-icon-button" data-variant="ghost" data-size="xs" :aria-label="t('composer.removeAttachment')" @click="removeImage(image.id)"><X aria-hidden="true" /></button>
        </span>
        <span v-for="paste in pastes" :key="paste.id" class="draft-chip">
          <span>{{ t("composer.pastedText", { lines: paste.text.split("\n").length }) }}</span>
          <button type="button" class="ui-icon-button" data-variant="ghost" data-size="xs" :aria-label="t('composer.removeAttachment')" @click="removePaste(paste.id)"><X aria-hidden="true" /></button>
        </span>
        <span v-for="file in files" :key="file.id" class="draft-chip" :title="file.name"><FileText :size="14" aria-hidden="true" /><span class="draft-file-name">{{ file.name }}</span><button type="button" class="ui-icon-button" data-variant="ghost" data-size="xs" :aria-label="t('composer.removeFile', { name: file.name })" @click="files = files.filter((item) => item.id !== file.id)"><X aria-hidden="true" /></button></span>
      </div>
      <p v-if="draft.reading" role="status" class="attachment-status">{{ t("composer.readingFiles") }}</p>
      <div v-if="menuOpen" :id="menuId" class="command-menu ui-popover" role="listbox" :aria-label="t('composer.commands')">
        <button
          v-for="(command, index) in matches"
          :id="`${menuId}-${index}`"
          :key="command.name"
          type="button"
          role="option"
          class="ui-menu-item command-item"
          :aria-selected="index === activeIndex"
          :data-highlighted="index === activeIndex ? '' : undefined"
          @mousedown.prevent
          @mouseenter="activeIndex = index"
          @click.stop="runMenuEntry(command)"
        >
          <span class="command-category">{{ index === 0 || matches[index - 1]?.category !== command.category ? t(`composer.category.${command.category}`) : '' }}</span>
          <span class="command-name">/{{ command.name }}<span v-if="command.name === 'branch'" class="command-argument"> &lt;prompt&gt;</span><span v-else-if="command.name === 'goal' || command.name === 'effort'" class="command-argument"> [args]</span></span>
          <span class="command-description">{{ command.category === 'skill' ? command.description : t(`composer.command.${command.name}`) }}</span>
        </button>
        <p class="command-keys">{{ t("composer.commandKeys") }}</p>
      </div>
      <div v-if="mentionShown" :id="mentionMenuId" class="command-menu ui-popover" role="listbox" :aria-label="t('composer.files')">
        <button
          v-for="(candidate, index) in candidates"
          :key="candidate.absolutePath"
          type="button"
          role="option"
          class="ui-menu-item command-item"
          :aria-selected="index === candidateActive"
          :data-highlighted="index === candidateActive ? '' : undefined"
          @mousedown.prevent
          @mouseenter="candidateActive = index"
          @click.stop="acceptCandidate(candidate)"
        >
          <span class="command-name">{{ candidate.relativePath }}<template v-if="candidate.dir">/</template></span>
        </button>
        <p class="command-keys">{{ t("composer.mentionKeys") }}</p>
      </div>
      <textarea
        ref="field"
        v-model="text"
        class="composer-input"
        rows="2"
        :placeholder="placeholder ?? t('composer.sendMessage')"
        :aria-label="placeholder ?? t('composer.sendMessage')"
        :aria-controls="menuOpen ? menuId : mentionShown ? mentionMenuId : undefined"
        :aria-expanded="menuOpen || mentionShown"
        :aria-activedescendant="menuOpen ? `${menuId}-${activeIndex}` : undefined"
        @input="resize(); updateMention()"
        @keydown="onKeydown"
        @keyup="updateMention"
        @click="updateMention"
        @paste="onPaste"
        @focus="focused = true"
        @blur="focused = false"
        @compositionstart="composing = true"
        @compositionend="composing = false"
      />
      <footer class="composer-toolbar">
        <div class="composer-tool-group">
          <input v-if="attachments" ref="upload" type="file" class="sr-only" tabindex="-1" :aria-label="t('composer.attachFiles')" :accept="FILE_ACCEPT" multiple @change="chooseFiles" />
          <button v-if="attachments" type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :disabled="sending || !!draft.reading" :aria-label="t('composer.attachFiles')" v-tooltip="t('composer.attachFiles')" @click="upload?.click()"><Paperclip aria-hidden="true" /></button>
          <button v-if="searchFiles" type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('composer.workspaceFiles')" v-tooltip="t('composer.workspaceFiles')" @click="fileDialog?.open()"><FileSearch aria-hidden="true" /></button>
          <ComposerModeMenu v-if="workModes.length && variant !== 'panel'" ref="modeMenu" :modes="workModes" :current="workMode" :policy="permission?.policy" :disabled="changingMode" @select="onMode" />
        </div>
        <div class="composer-run-group">
          <span v-if="contextLabel" class="composer-context" :data-near="contextNearLimit ? '' : undefined" v-tooltip="t('composer.contextUsage')">{{ contextLabel }}</span>
          <ComposerModelMenu v-if="variant !== 'panel'" ref="modelMenu" :model="model" :selectable="modelSelectable" :disabled="changingModel || changingEffort" :effort="reasoningEffort" :effort-selectable="selectEffort !== undefined" @select="onModel" @effort="onEffort" />
          <button
            class="ui-icon-button"
            :data-variant="showStop ? 'contrast' : 'primary'"
            data-size="sm"
            type="button"
            :aria-label="showStop ? t('composer.stopResponse') : t('composer.send')"
            v-tooltip="showStop ? t('composer.stopResponse') : t('composer.send')"
            :disabled="showStop ? stopping : !canSend"
            @click.stop="showStop ? stop() : submit()"
          >
            <Square v-if="showStop" fill="currentColor" aria-hidden="true" />
            <ArrowUp v-else :stroke-width="2.25" aria-hidden="true" />
          </button>
        </div>
      </footer>
    </div>
    <div v-if="$slots.status" class="composer-statusbar">
      <div class="composer-status-start"><slot name="status" /></div>
    </div>
  </section>
  <WorkspaceFileDialog v-if="searchFiles" ref="fileDialog" :search="searchFiles" @select="insertFile" />
</template>

<style scoped>
.draft-file-name { max-width: 220px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.attachment-status { padding: var(--space-2); color: var(--muted-foreground); font-size: var(--text-sm); }
.composer-toolbar { flex-wrap: wrap; }
.composer-context { color: var(--muted-foreground); font-size: var(--text-xs); font-variant-numeric: tabular-nums; white-space: nowrap; }
.composer-context[data-near] { color: var(--danger); }
.composer-run-group { margin-left: auto; max-width: 100%; flex-wrap: wrap; }
.composer-input { display: block; width: 100%; min-height: 60px; max-height: 300px; resize: none; padding: 0; border: 0; background: transparent; color: var(--foreground); font: var(--text-base)/var(--leading-relaxed) var(--font-sans); outline: none; }
.composer-input::placeholder { color: var(--muted-foreground); }
.composer-panel[data-variant="panel"] { padding: var(--space-2) var(--space-3) var(--space-3); }
.composer-panel[data-variant="panel"] .composer-input { min-height: 48px; }
.command-menu { position: absolute; bottom: calc(100% + var(--space-2)); left: 0; width: 100%; max-width: none; max-height: min(320px, 50dvh); overflow-y: auto; transform-origin: bottom left; --popover-enter-y: 4px; }
.command-item { display: grid; grid-template-columns: 48px 128px minmax(0, 1fr); }
.command-category, .command-description { color: var(--muted-foreground); }
.command-category { font-size: var(--text-xs); }
.command-name { font-family: var(--font-mono); }
.command-argument { color: var(--muted-foreground); font-size: var(--text-xs); }
.draft-attachments { display: flex; flex-wrap: wrap; gap: var(--space-2); padding: var(--space-2) var(--space-3) 0; }
.draft-chip { display: inline-flex; min-width: 0; align-items: center; gap: var(--space-1); padding: 2px var(--space-1) 2px var(--space-2); border: 1px solid var(--border); border-radius: var(--control-sm-radius); background: var(--muted); font-size: var(--text-xs); }
.draft-thumb { width: 36px; height: 36px; border-radius: var(--control-xs-radius); object-fit: cover; }
.command-keys { margin: var(--space-1) 0 0; padding: var(--space-2); border-top: 1px solid var(--border); color: var(--muted-foreground); font-size: var(--text-xs); }

</style>
