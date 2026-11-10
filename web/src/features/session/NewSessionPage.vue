<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue"
import { useRoute, useRouter } from "vue-router"
import { useI18n } from "vue-i18n"
import { Check, ChevronDown, Folder, GitFork, Network, Search } from "lucide-vue-next"
import { canChooseWorkspace, chooseWorkspace } from "../../desktop"
import BrandMark from "../../components/BrandMark.vue"
import ComposerPanel from "./composer/ComposerPanel.vue"
import { COORDINATOR_PROMPT, COORDINATOR_TOOLS } from "./coordinator"
import { readSkills } from "../../rpc/general-settings"
import { MAIN_BRANCH, type DraftAttachments, type ModelId, type PermissionPreset } from "../../rpc/protocol"
import { useChatStore } from "../../stores/chat"
import { useConnectionStore } from "../../stores/connection"
import { useGitStore } from "../../stores/git"
import { useModelsStore } from "../../stores/models"
import { datedName, isAbsolutePath, useSessionsStore, workspaceName } from "../../stores/sessions"
import { useUiStore } from "../../stores/ui"

/**
 * A new session: pick a recent folder, type an absolute directory or choose no
 * folder, write the first message, and send. Sending creates the session there
 * (without a folder, in a scratch directory of its own), submits the message
 * and opens it. In a Git repository the session may start in a new worktree on
 * its own branch instead.
 */
const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const sessions = useSessionsStore()
const chat = useChatStore()
const models = useModelsStore()
const connection = useConnectionStore()
const git = useGitStore()
const planning = ref(false)
/** Where the preset last chosen here is kept, so the next new Session starts with it. */
const PRESET_KEY = "dim.newSession.preset"
const preset = ref<PermissionPreset>(storedPreset() ?? "workspace-write")
const permissionControl = computed(() => connection.supports("session.permission.get") && connection.supports("session.permission.update")
  ? { policy: { preset: preset.value, allowedTools: [], disallowedTools: [] }, select: async (next: PermissionPreset) => { choosePreset(next) } }
  : undefined)
const menu = ref<HTMLDetailsElement>()
const search = ref<HTMLInputElement>()
const composer = ref<InstanceType<typeof ComposerPanel>>()
const ui = useUiStore()
// A shortcut opens a menu of the composer.
watch(() => ui.composerMenu, (request) => {
  if (request?.pane !== "main") return
  ui.composerMenu = undefined
  composer.value?.openMenu(request.menu)
}, { flush: "sync" })
let active = true
onBeforeUnmount(() => { active = false })

/** The directory the session will run in, seeded from `?cwd=` or the latest workspace; empty for no folder. */
const cwd = ref(typeof route.query.cwd === "string" ? route.query.cwd : "")
/** Whether `cwd` is a choice already, which the latest workspace must not replace. */
let seeded = typeof route.query.cwd === "string"
// A scratch directory handed over by another Session stands for no folder.
watch(() => sessions.isScratch(cwd.value), (scratch) => { if (scratch) cwd.value = "" }, { immediate: true })
const inRepository = computed(() => git.places[cwd.value]?.root !== undefined)
const worktree = ref(false)
const branch = ref("")
/** Start a coordinator, which delegates every piece of work to team members. */
const coordinator = ref(false)
/** The branch a new worktree gets when none is typed. */
const suggestedBranch = `lacus/${datedName()}`
watch(cwd, (dir) => { if (dir !== "") void git.inspect([dir]) }, { immediate: true })
const listSkills = computed(() => {
  if (!connection.supports("skills.catalog")) return undefined
  const scope = cwd.value === "" ? {} : { cwd: cwd.value }
  return async () => (await connection.client.call("skills.catalog", scope, readSkills)).entries.filter((skill) => skill.enabled && skill.shadowedBy === undefined)
})
/** What the picker's search field holds: a filter for the recent folders, or an absolute directory to use. */
const typed = ref("")
const choosing = ref(false)
const pickerError = ref("")
watch(() => sessions.workspaces, (workspaces) => {
  if (seeded || workspaces[0] === undefined) return
  seeded = true
  cwd.value = workspaces[0].cwd
}, { immediate: true })
/** Where no native picker exists, typing the directory is the way to a new folder. */
const searchLabel = computed(() => t(canChooseWorkspace ? "newThread.searchFolders" : "newThread.searchOrPath"))
const query = computed(() => typed.value.trim().toLowerCase())
const folders = computed(() => sessions.workspaces.filter((workspace) => workspace.cwd !== ""
  && (workspace.name.toLowerCase().includes(query.value) || workspace.cwd.toLowerCase().includes(query.value))))
/** No folder is on offer where the service makes scratch directories. */
const offersNone = computed(() => sessions.scratchRoot !== undefined && t("workspace.none").toLowerCase().includes(query.value))
const typedPath = computed(() => {
  const path = typed.value.trim()
  return isAbsolutePath(path) && !folders.value.some((workspace) => workspace.cwd === path) ? path : ""
})

/** The model picked here; until then, the directory's default. */
const picked = ref<ModelId>()
const fallback = ref<ModelId>()
watch(cwd, async (dir) => {
  fallback.value = undefined
  if (dir !== "") sessions.rememberWorkspace(dir)
  const model = await models.defaultFor(dir)
  if (cwd.value === dir) fallback.value = model
}, { immediate: true })
const model = computed(() => picked.value ?? fallback.value)
const reasoningEffort = ref<string>()
watch([() => model.value?.providerId, () => model.value?.modelId], () => { reasoningEffort.value = undefined })

function choose(dir: string): void {
  seeded = true
  cwd.value = dir
  if (menu.value) menu.value.open = false
}

/** Enter in the search field takes the typed directory, else the first match. */
function chooseTyped(): void {
  const dir = typedPath.value || folders.value[0]?.cwd
  if (dir !== undefined) choose(dir)
}

async function onMenuToggle(): Promise<void> {
  if (!menu.value?.open) { typed.value = ""; return }
  await nextTick()
  search.value?.focus()
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
  if (cwd.value === "" && sessions.scratchRoot === undefined) throw new Error(t("newThread.workspaceRequired"))
  const chosen = cwd.value
  const directory = worktree.value && inRepository.value ? (await git.createWorktree(chosen, branch.value.trim() || suggestedBranch)).path : chosen
  // A worktree Session is listed under its main repository, which replaces a typed alias of it.
  if (directory !== chosen) sessions.rememberWorkspace(git.places[chosen]?.main ?? chosen, chosen)
  const selected = picked.value ?? (reasoningEffort.value === undefined ? undefined : model.value)
  const sessionId = await sessions.create(directory, selected, reasoningEffort.value, coordinator.value ? { tools: COORDINATOR_TOOLS, appendSystemPrompt: COORDINATOR_PROMPT } : undefined)
  if (coordinator.value) ui.markCoordinator(sessionId)
  chat.open(sessionId)
  // The preset first: plan mode keeps the preset it replaces to restore on approval.
  if (permissionControl.value !== undefined) await chat.setPermission(sessionId, preset.value)
  if (planning.value) await chat.setPlanning(sessionId, true)
  await chat.submit(sessionId, MAIN_BRANCH, text, attachments)
  if (active && cwd.value === chosen) await router.push({ name: "chat-session", params: { sessionId } })
}
</script>

<template>
  <section class="new-session-page">
    <div class="new-session-hero">
      <BrandMark class="new-session-mark" />
      <h1 data-route-heading tabindex="-1">{{ t("newThread.title") }}</h1>
    </div>
    <ComposerPanel ref="composer" class="is-new-thread" :placeholder="t('composer.describeTask')" :draft-key="`new:${cwd}`" :running="false" :model="model" :model-selectable="true" :send="send" :stores-pastes="true" :select-model="(next) => { picked = next }" :reasoning-effort="reasoningEffort" :select-effort="(value) => { reasoningEffort = value }" :list-skills="listSkills" :permission="permissionControl" :plan-mode="{ active: planning, toggle: () => { planning = !planning } }">
      <template #status>
        <details ref="menu" class="composer-popover workspace-menu" @toggle="onMenuToggle">
          <summary class="ui-button" data-variant="ghost" data-size="xs" data-trailing v-tooltip="cwd || undefined">
            <Folder aria-hidden="true" />
            <span class="workspace-trigger-label">{{ cwd ? workspaceName(cwd) : t("newThread.selectWorkspace") }}</span>
            <ChevronDown class="workspace-chevron" aria-hidden="true" />
          </summary>
          <div class="composer-popover-content workspace-menu-content ui-popover" data-width="lg">
            <label class="ui-menu-search">
              <Search aria-hidden="true" />
              <input ref="search" v-model="typed" type="text" spellcheck="false" autocapitalize="off" :placeholder="searchLabel" :aria-label="searchLabel" @keydown.enter.prevent="chooseTyped">
            </label>
            <p v-if="pickerError" class="ui-banner" data-tone="danger" role="alert">{{ pickerError }}</p>
            <div class="workspace-list" role="listbox" :aria-label="t('sidebar.workspaces')">
              <button v-if="typedPath" type="button" role="option" class="ui-menu-item" aria-selected="false" @click="choose(typedPath)">
                <span>{{ t("newThread.usePath", { path: typedPath }) }}</span>
              </button>
              <button v-if="offersNone" type="button" role="option" class="ui-menu-item" :aria-selected="cwd === ''" @click="choose('')">
                <span>{{ t("workspace.none") }}</span>
                <Check v-if="cwd === ''" class="ui-menu-check" aria-hidden="true" />
              </button>
              <p v-if="folders.length" class="ui-menu-label">{{ t("newThread.recent") }}</p>
              <button
                v-for="workspace in folders"
                :key="workspace.cwd"
                type="button"
                role="option"
                class="ui-menu-item"
                :title="workspace.cwd"
                :aria-selected="workspace.cwd === cwd"
                @click="choose(workspace.cwd)"
              >
                <span>{{ workspace.name }}</span>
                <Check v-if="workspace.cwd === cwd" class="ui-menu-check" aria-hidden="true" />
              </button>
              <p v-if="!typedPath && !offersNone && !folders.length" class="menu-empty">{{ t("newThread.noMatch") }}</p>
            </div>
            <template v-if="canChooseWorkspace">
              <hr class="ui-menu-separator">
              <button type="button" class="ui-menu-item" :disabled="choosing" @click="browse">{{ t("newThread.browseWorkspace") }}</button>
            </template>
          </div>
        </details>
        <button type="button" class="ui-button" data-variant="ghost" data-size="xs" :aria-pressed="coordinator" v-tooltip="t('coordinator.hint')" @click="coordinator = !coordinator">
          <Network aria-hidden="true" /><span>{{ t("coordinator.label") }}</span>
        </button>
        <template v-if="inRepository">
          <button type="button" class="ui-button" data-variant="ghost" data-size="xs" :aria-pressed="worktree" v-tooltip="t('git.newWorktreeHint')" @click="worktree = !worktree">
            <GitFork aria-hidden="true" /><span>{{ t("git.newWorktree") }}</span>
          </button>
          <input v-if="worktree" v-model="branch" class="ui-field worktree-branch" data-size="xs" type="text" spellcheck="false" autocapitalize="off" :placeholder="suggestedBranch" :aria-label="t('git.branchName')">
        </template>
      </template>
    </ComposerPanel>
  </section>
</template>

<style scoped>
.workspace-trigger-label { max-width: 240px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.workspace-chevron { transition: transform var(--duration-press) var(--ease-out); }
.workspace-menu[open] .workspace-chevron { transform: rotate(180deg); }
.workspace-menu-content { display: flex; flex-direction: column; }
.workspace-list { min-height: 0; max-height: 240px; overflow-x: hidden; overflow-y: auto; }
.ui-button[aria-pressed="true"] { background: var(--hover); color: var(--foreground); }
.worktree-branch { width: 180px; font-family: var(--font-mono); }
</style>
