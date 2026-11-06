<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue"
import { useRoute, useRouter } from "vue-router"
import { useI18n } from "vue-i18n"
import { Check, ChevronDown, Folder, FolderOpen } from "lucide-vue-next"
import { canChooseWorkspace, chooseWorkspace } from "../../desktop"
import BrandMark from "../../components/BrandMark.vue"
import ComposerPanel from "./ComposerPanel.vue"
import { readSkills } from "../../rpc/general-settings"
import { MAIN_BRANCH, type DraftAttachments, type ModelId, type PermissionPreset } from "../../rpc/protocol"
import { useChatStore } from "../../stores/chat"
import { useConnectionStore } from "../../stores/connection"
import { useModelsStore } from "../../stores/models"
import { useSessionsStore, workspaceName } from "../../stores/sessions"

/**
 * A new session: pick a known workspace or type an absolute directory, write
 * the first message, and send. Sending creates the session there, submits the
 * message and opens it.
 */
const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const sessions = useSessionsStore()
const chat = useChatStore()
const models = useModelsStore()
const connection = useConnectionStore()
const planning = ref(false)
/** Where the preset last chosen here is kept, so the next new Session starts with it. */
const PRESET_KEY = "dim.newSession.preset"
const preset = ref<PermissionPreset>(storedPreset() ?? "workspace-write")
const permissionControl = computed(() => connection.supports("session.permission.get") && connection.supports("session.permission.update")
  ? { policy: { preset: preset.value, allowedTools: [], disallowedTools: [] }, select: async (next: PermissionPreset) => { choosePreset(next) } }
  : undefined)
const menu = ref<HTMLDetailsElement>()
let active = true
onBeforeUnmount(() => { active = false })

/** The directory the session will run in, seeded from `?cwd=` or the latest workspace. */
const cwd = ref(typeof route.query.cwd === "string" ? route.query.cwd : "")
const listSkills = computed(() => {
  if (cwd.value === "" || !connection.supports("skills.catalog")) return undefined
  const dir = cwd.value
  return async () => (await connection.client.call("skills.catalog", { cwd: dir }, readSkills)).entries.filter((skill) => skill.enabled && skill.shadowedBy === undefined)
})
const typed = ref("")
const choosing = ref(false)
const pickerError = ref("")
watch(() => sessions.workspaces, (workspaces) => {
  if (cwd.value === "" && workspaces[0] !== undefined) cwd.value = workspaces[0].cwd
}, { immediate: true })

/** The model picked here; until then, the directory's default. */
const picked = ref<ModelId>()
const fallback = ref<ModelId>()
watch(cwd, async (dir) => {
  fallback.value = undefined
  if (dir === "") return
  sessions.rememberWorkspace(dir)
  const model = await models.defaultFor(dir)
  if (cwd.value === dir) fallback.value = model
}, { immediate: true })
const model = computed(() => picked.value ?? fallback.value)
const reasoningEffort = ref<string>()
watch([() => model.value?.providerId, () => model.value?.modelId], () => { reasoningEffort.value = undefined })

function choose(dir: string): void {
  cwd.value = dir
  if (menu.value) menu.value.open = false
}

function storedPreset(): PermissionPreset | undefined {
  try {
    const value = localStorage.getItem(PRESET_KEY)
    return value === "read-only" || value === "ask" || value === "workspace-write" || value === "full-access" ? value : undefined
  } catch { return undefined }
}

function choosePreset(next: PermissionPreset): void {
  preset.value = next
  try { localStorage.setItem(PRESET_KEY, next) } catch { /* the choice still applies to this Session */ }
}

function useTyped(): void {
  const dir = typed.value.trim()
  if (dir === "") return
  choose(dir)
  typed.value = ""
}

async function browse(): Promise<void> {
  if (choosing.value) return
  choosing.value = true
  pickerError.value = ""
  try {
    const dir = await chooseWorkspace(cwd.value)
    if (dir !== null) choose(dir)
  } catch (cause) {
    pickerError.value = cause instanceof Error ? cause.message : String(cause)
  } finally {
    choosing.value = false
  }
}

async function send(text: string, attachments: DraftAttachments): Promise<void> {
  if (cwd.value === "") throw new Error(t("newThread.workspaceRequired"))
  const directory = cwd.value
  const selected = picked.value ?? (reasoningEffort.value === undefined ? undefined : model.value)
  const sessionId = await sessions.create(directory, selected, reasoningEffort.value)
  chat.open(sessionId)
  // The preset first: plan mode keeps the preset it replaces to restore on approval.
  if (permissionControl.value !== undefined) await chat.setPermission(sessionId, preset.value)
  if (planning.value) await chat.setPlanning(sessionId, true)
  await chat.submit(sessionId, MAIN_BRANCH, text, attachments)
  if (active && cwd.value === directory) await router.push({ name: "chat-session", params: { sessionId } })
}
</script>

<template>
  <section class="new-session-page">
    <div class="new-session-hero">
      <BrandMark class="new-session-mark" />
      <h1 data-route-heading tabindex="-1">{{ t("newThread.title") }}</h1>
    </div>
    <ComposerPanel class="is-new-thread" :draft-key="`new:${cwd}`" :running="false" :model="model" :model-selectable="true" :send="send" :stores-pastes="true" :select-model="(next) => { picked = next }" :reasoning-effort="reasoningEffort" :select-effort="(value) => { reasoningEffort = value }" :list-skills="listSkills" :permission="permissionControl" :plan-mode="{ active: planning, toggle: () => { planning = !planning } }">
      <template #status>
        <details ref="menu" class="composer-popover workspace-menu">
          <summary class="ui-button" data-variant="ghost" data-size="xs" data-trailing v-tooltip="cwd || undefined">
            <Folder aria-hidden="true" />
            <span class="workspace-trigger-label">{{ cwd ? workspaceName(cwd) : t("newThread.selectWorkspace") }}</span>
            <ChevronDown class="workspace-chevron" aria-hidden="true" />
          </summary>
          <div class="composer-popover-content workspace-menu-content ui-popover" data-width="lg">
            <button v-if="canChooseWorkspace" type="button" class="ui-menu-item" :disabled="choosing" @click="browse">
              <FolderOpen aria-hidden="true" />{{ t("newThread.browseWorkspace") }}
            </button>
            <p v-if="pickerError" class="ui-banner" data-tone="danger" role="alert">{{ pickerError }}</p>
            <form class="workspace-path-form" @submit.prevent="useTyped">
              <input v-model="typed" class="ui-field" data-size="sm" type="text" spellcheck="false" autocapitalize="off" :placeholder="t('newThread.pathPlaceholder')" :aria-label="t('newThread.pathLabel')">
              <button type="submit" class="ui-button" data-variant="secondary" data-size="sm" :disabled="typed.trim() === ''">{{ t("newThread.usePath") }}</button>
            </form>
            <hr v-if="sessions.workspaces.length" class="ui-menu-separator">
            <div class="workspace-list" role="listbox" :aria-label="t('sidebar.workspaces')">
              <button
                v-for="workspace in sessions.workspaces"
                :key="workspace.cwd"
                type="button"
                role="option"
                class="ui-menu-item"
                data-two-line
                :aria-selected="workspace.cwd === cwd"
                @click="choose(workspace.cwd)"
              >
                <Folder aria-hidden="true" />
                <span>{{ workspace.name }}<span class="ui-menu-desc workspace-path">{{ workspace.cwd }}</span></span>
                <Check v-if="workspace.cwd === cwd" class="ui-menu-check" aria-hidden="true" />
              </button>
            </div>
          </div>
        </details>
      </template>
    </ComposerPanel>
  </section>
</template>

<style scoped>
.workspace-trigger-label { max-width: 240px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.workspace-chevron { transition: transform var(--duration-press) var(--ease-out); }
.workspace-menu[open] .workspace-chevron { transform: rotate(180deg); }
.workspace-menu-content { display: flex; flex-direction: column; }
.workspace-path-form { display: flex; gap: var(--space-1); padding: var(--space-1); }
.workspace-path-form > .ui-field { min-width: 0; flex: 1; font-family: var(--font-mono); }
.workspace-list { min-height: 0; max-height: 240px; overflow-x: hidden; overflow-y: auto; }
.workspace-path { overflow: hidden; font-family: var(--font-mono); text-overflow: ellipsis; white-space: nowrap; }
</style>
