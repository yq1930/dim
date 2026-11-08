<script setup lang="ts">
import { computed, nextTick, onMounted, ref, useId, watch } from "vue"
import { useRoute, useRouter } from "vue-router"
import { useI18n } from "vue-i18n"
import { ArrowUp, FileSearch, FileText, Paperclip, Square, X } from "lucide-vue-next"
import type { ContextUsage, DraftAttachments, FileCandidate, ModelId, PermissionPreset, PermissionPolicy } from "../../rpc/protocol"
import { formatTokens } from "./usage-format"
import { linkMentions } from "../../mentions"
import { useModelsStore } from "../../stores/models"
import { useSessionsStore } from "../../stores/sessions"
import ComposerModeMenu, { type WorkMode } from "./ComposerModeMenu.vue"
import ComposerModelMenu from "./ComposerModelMenu.vue"
import WorkspaceFileDialog from "./WorkspaceFileDialog.vue"
import { FILE_ACCEPT, useComposerAttachments } from "./composer-attachments"
import { ARGUMENT_COMMANDS, commandWords, useCommandMenu, type MenuEntry } from "./composer-commands"
import { createComposerDraft, useComposerDrafts } from "./composer-drafts"
import { useHistoryRecall } from "./composer-history"
import { useMentionPicker } from "./composer-mentions"
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
  /** Receives the words after `/review`, the focus of the review; empty reviews everything. */
  review?: (words: string) => Promise<void>
  status?: () => void
  copy?: () => Promise<void>
  diff?: () => void
  tasks?: () => void
}

/**
 * The message composer: a growing text field, the model menu, and one button
 * that sends, or stops the run while one is in flight. Enter sends,
 * Shift+Enter breaks the line, Escape stops a run, Shift+Tab steps through the
 * work modes and ↑/↓ recall the line's earlier messages into an empty draft.
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
  /** The line's earlier messages, oldest first, as sent. */
  history?: readonly string[]
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
/** Shift+Tab steps through these, as the CLI cycles its permission modes; Auto and Read only are chosen in the menu. */
const cycleModes = computed<WorkMode[]>(() => workModes.value.filter((mode) => mode === "ask" || mode === "workspace-write" || mode === "plan"))
/** Read out once a mode changes by keyboard. */
const announcement = ref("")
const error = computed({ get: () => draft.value.error, set: (value) => { draft.value.error = value } })
const composing = ref(false)
const focused = ref(false)
const dismissed = ref(false)
const { images, pastes, files, empty: noAttachments, onPaste, onDrop, choose: chooseFiles, removeImage, removePaste, removeFile } = useComposerAttachments(draft, { enabled: () => props.attachments, storesPastes: () => props.storesPastes === true })
const { candidates, active: candidateActive, open: mentionOpen, shown: mentionShown, update: updateMention, accept: acceptCandidate, insert: insertFile, move: moveCandidate, dismiss: dismissMention, reset: resetMention } = useMentionPicker({ field, draft, focused, search: () => props.searchFiles, resize })
const { recall, forget: forgetRecall } = useHistoryRecall({ field, draft, history: () => props.history ?? [], resize })
const mentionMenuId = useId()
watch(() => props.draftKey, () => { resetMention(); forgetRecall(); fileDialog.value?.close(); void nextTick(resize) })

const { matches, commandMode, open: menuOpen, active: activeIndex, move: moveCommand } = useCommandMenu({
  text,
  focused,
  composing,
  dismissed,
  commands: () => [
    { category: "model", name: "models", available: props.modelSelectable && models.available && !changingModel.value },
    { category: "session", name: "branch", available: props.createBranch !== undefined },
    { category: "session", name: "compact", available: props.sessionCommands?.compact !== undefined },
    { category: "session", name: "usage", available: props.sessionCommands?.usage !== undefined },
    { category: "session", name: "fork", available: props.sessionCommands?.fork !== undefined },
    { category: "session", name: "goal", available: props.sessionCommands?.goal !== undefined },
    { category: "session", name: "review", available: props.sessionCommands?.review !== undefined },
    { category: "session", name: "diff", available: props.sessionCommands?.diff !== undefined },
    { category: "session", name: "tasks", available: props.sessionCommands?.tasks !== undefined },
    { category: "session", name: "status", available: props.sessionCommands?.status !== undefined },
    { category: "session", name: "copy", available: props.sessionCommands?.copy !== undefined },
    { category: "model", name: "effort", available: props.sessionCommands?.effort !== undefined },
    { category: "model", name: "permissions", available: props.permission !== undefined && props.variant !== "panel" },
    { category: "session", name: "lines", available: props.openLines !== undefined },
    { category: "session", name: "plan", available: props.planMode !== undefined },
    { category: "session", name: "new", available: true },
  ],
  listSkills: () => props.listSkills,
})
const contextLabel = computed(() => {
  const reading = props.contextUsage
  return reading === undefined || reading.contextWindow <= 0 ? undefined : `${formatTokens(reading.estimatedInputTokens)} / ${formatTokens(reading.contextWindow)}`
})
const contextNearLimit = computed(() => {
  const reading = props.contextUsage
  return reading !== undefined && reading.thresholdTokens > 0 && reading.estimatedInputTokens >= reading.thresholdTokens * 0.9
})
const showStop = computed(() => props.running && props.stop !== undefined && text.value.trim() === "" && noAttachments.value)
const canSend = computed(() => (text.value.trim() !== "" || !noAttachments.value) && !sending.value && !draft.value.reading && !changingModel.value && !changingEffort.value && !changingMode.value)

function resize(): void {
  const element = field.value
  if (!element) return
  element.style.height = "auto"
  element.style.height = `${Math.min(element.scrollHeight, 300)}px`
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
  const branchWords = dismissed.value ? undefined : commandWords(message, "branch")
  if (branchWords === "") {
    await runCommand("branch")
    return
  }
  for (const name of ["goal", "review", "effort"] as const) {
    const handler = props.sessionCommands?.[name]
    const words = dismissed.value || handler === undefined ? undefined : commandWords(message, name)
    if (handler === undefined || words === undefined) continue
    text.value = ""
    await nextTick()
    resize()
    await handler(words)
    return
  }
  sending.value = true
  error.value = ""
  try {
    if (branchWords !== undefined) {
      if (!props.createBranch) throw new Error(t("composer.branchUnavailable"))
      await props.createBranch(branchWords)
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
    forgetRecall()
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

async function cycleMode(): Promise<void> {
  const modes = cycleModes.value
  const current = workMode.value === undefined ? -1 : modes.indexOf(workMode.value)
  const next = modes[(current + 1) % modes.length]!
  await onMode(next)
  if (error.value === "") announcement.value = t("composer.mode.switched", { mode: t(`composer.mode.${next}`) })
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
  // WebKit takes a clipboard write only within the key press or click that asked for it.
  const copying = name === "copy" ? props.sessionCommands?.copy?.() : undefined
  const argument = ARGUMENT_COMMANDS.includes(name)
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
  else if (name === "copy") await copying
  else if (name === "compact" || name === "usage" || name === "fork") await props.sessionCommands?.[name]?.()
  else if (name === "status" || name === "diff" || name === "tasks") props.sessionCommands?.[name]?.()
}

function onKeydown(event: KeyboardEvent): void {
  if (event.isComposing || composing.value || event.keyCode === 229) return
  if (mentionOpen.value && candidates.value.length > 0) {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault()
      moveCandidate(event.key === "ArrowDown" ? 1 : -1)
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
    dismissMention()
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
    moveCommand(event.key === "ArrowDown" ? 1 : -1)
    return
  }
  const plain = !event.altKey && !event.ctrlKey && !event.metaKey
  if (event.key === "Tab" && event.shiftKey && plain && props.variant !== "panel" && cycleModes.value.length > 1) {
    event.preventDefault()
    if (!changingMode.value) void cycleMode()
    return
  }
  // Escape closes an open menu first (AppShell); otherwise it stops the run, as the CLI's Esc interrupts.
  if (event.key === "Escape" && plain && props.running && props.stop !== undefined && !props.stopping && document.querySelector("details.composer-popover[open]") === null) {
    event.preventDefault()
    void stop()
    return
  }
  if ((event.key === "ArrowUp" || event.key === "ArrowDown") && plain && !event.shiftKey && recall(event.key === "ArrowUp" ? -1 : 1)) {
    event.preventDefault()
    return
  }
  if (event.key === "Enter" && !event.shiftKey && !event.altKey) {
    event.preventDefault()
    void submit()
  }
}

/** Puts `block` at the end of the draft as a paragraph of its own and focuses the field. */
async function insert(block: string): Promise<void> {
  const gap = text.value === "" || text.value.endsWith("\n\n") ? "" : text.value.endsWith("\n") ? "\n" : "\n\n"
  text.value = `${text.value}${gap}${block}`
  forgetRecall()
  await nextTick()
  resize()
  const element = field.value
  element?.focus()
  element?.setSelectionRange(text.value.length, text.value.length)
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
/** Opens the mode or model menu, as their shortcuts do. */
function openMenu(menu: "mode" | "model"): void {
  if (menu === "mode") modeMenu.value?.open()
  else modelMenu.value?.open()
}

defineExpose({ focus: () => field.value?.focus(), quote, insert, openMenu })
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
        <span v-for="file in files" :key="file.id" class="draft-chip" :title="file.name"><FileText :size="14" aria-hidden="true" /><span class="draft-file-name">{{ file.name }}</span><button type="button" class="ui-icon-button" data-variant="ghost" data-size="xs" :aria-label="t('composer.removeFile', { name: file.name })" @click="removeFile(file.id)"><X aria-hidden="true" /></button></span>
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
          <span class="command-name">/{{ command.name }}<span v-if="command.name === 'branch'" class="command-argument"> &lt;prompt&gt;</span><span v-else-if="command.name === 'goal' || command.name === 'effort' || command.name === 'review'" class="command-argument"> [args]</span></span>
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
    <span class="sr-only" role="status" aria-live="polite">{{ announcement }}</span>
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
