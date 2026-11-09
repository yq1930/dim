<script setup lang="ts">
import { computed, provide, ref, watch } from "vue"
import { RouterLink, useRoute, useRouter } from "vue-router"
import { useI18n } from "vue-i18n"
import { ArrowLeftRight, Menu, X } from "lucide-vue-next"
import BrandMark from "../../components/BrandMark.vue"
import ConfirmDialog from "../../components/ConfirmDialog.vue"
import ApprovalPrompt from "./composer/ApprovalPrompt.vue"
import ComposerPanel from "./composer/ComposerPanel.vue"
import OpenInMenu from "./header/OpenInMenu.vue"
import QueuedInputs from "./composer/QueuedInputs.vue"
import PlanReview from "./composer/PlanReview.vue"
import QuestionPrompt from "./composer/QuestionPrompt.vue"
import SessionPaneButtons from "./header/SessionPaneButtons.vue"
import SessionTitleMenu from "./header/SessionTitleMenu.vue"
import TimelineView from "./transcript/TimelineView.vue"
import TranscriptViewMenu from "./header/TranscriptViewMenu.vue"
import SessionOutline from "./header/SessionOutline.vue"
import SessionCrumb from "./header/SessionCrumb.vue"
import TodoPanel from "../panes/plan/TodoPanel.vue"
import TurnChangesDialog from "./transcript/TurnChangesDialog.vue"
import { useComposerDrafts } from "./composer/composer-drafts"
import { LINK_CONTEXT } from "../../components/link-context"
import { useSessionCommands } from "./session-commands"
import { MAIN_BRANCH, type ContextUsage, type DraftAttachments, type ModelId, type PermissionPreset } from "../../rpc/protocol"
import { readSkills } from "../../rpc/general-settings"
import { useChatStore } from "../../stores/chat"
import { useConnectionStore } from "../../stores/connection"
import { useModelsStore } from "../../stores/models"
import { useSessionsStore } from "../../stores/sessions"
import { useToastStore } from "../../stores/toast"
import { usePanesStore } from "../../stores/panes"
import { useUiStore, type Pane } from "../../stores/ui"

/** A Session's transcript and composer; `pane` is "split" for the one shown beside the routed Session. */
const props = defineProps<{ sessionId: string; pane?: Pane }>()
const { t } = useI18n()
const models = useModelsStore()
const chat = useChatStore()
const connection = useConnectionStore()
const sessions = useSessionsStore()
const ui = useUiStore()
/** Used only in handlers: the chat-input test stubs the stores ChatPage reads while rendering. */
const panes = usePanesStore()
const toast = useToastStore()
const route = useRoute()
const router = useRouter()
const forkConfirm = ref<InstanceType<typeof ConfirmDialog>>()
const drafts = useComposerDrafts()

watch(() => props.sessionId, (id) => chat.open(id), { immediate: true })

const view = computed(() => chat.views[props.sessionId])
const summary = computed(() => sessions.byId.get(props.sessionId))
provide(LINK_CONTEXT, computed(() => summary.value === undefined ? undefined : { sessionId: props.sessionId, base: summary.value.cwd, filePane: props.pane !== "split" }))
const title = computed(() => summary.value?.title ?? summary.value?.preview ?? t("sidebar.untitled"))
watch(() => summary.value?.cwd, (cwd) => { if (cwd) sessions.rememberWorkspace(cwd) }, { immediate: true })
const approval = computed(() => view.value?.approvals.find((request) => request.branchId === MAIN_BRANCH))
const questions = computed(() => view.value?.questions.filter((ask) => ask.branchId === MAIN_BRANCH) ?? [])
const reviews = computed(() => view.value?.reviews.filter((review) => review.branchId === MAIN_BRANCH) ?? [])
const planControl = computed(() => {
  const session = view.value
  if (session?.status !== "ready" || !session.methods.includes("session.plan.set")) return undefined
  return { active: session.planning, toggle: () => setPlanning(!session.planning) }
})
const rulesAvailable = computed(() => connection.supports("permission.rules") && connection.supports("permission.set_rules"))
const storesPastes = computed(() => view.value?.methods.includes("attachment.put") === true)
const searchFiles = computed(() => view.value?.methods.includes("session.files.search") ? (query: string) => chat.searchFiles(props.sessionId, query) : undefined)
const fileChanges = computed(() => view.value?.methods.includes("session.files.changes") === true && view.value.methods.includes("session.files.rewind"))
const changesDialog = ref<InstanceType<typeof TurnChangesDialog>>()
const timeline = ref<InstanceType<typeof TimelineView>>()
const outline = computed(() => timeline.value?.markers ?? [])
const split = computed(() => props.pane === "split")
/** The main line's messages, oldest first and the queued ones last, for the composer's ↑ recall. */
const sentMessages = computed(() => [
  ...(view.value?.rows ?? []).flatMap((row) => row.kind === "user" ? [row.text] : []),
  ...(view.value?.queued ?? []).map((input) => input.text),
].filter((text) => text.trim() !== ""))
/** The App Server serves the permission methods on the connection; a Session may also list them. */
const permissionControl = computed(() => {
  const session = view.value
  const serves = (method: string) => connection.supports(method) || session?.methods.includes(method) === true
  if (session?.status !== "ready" || !serves("session.permission.get") || !serves("session.permission.update")) return undefined
  return { policy: session.permission, select: setPermission }
})
/** In split view only the pane the operator is in answers approvals from the keyboard. */
const approvalKeyboard = computed(() => split.value === (ui.activePane === "split") && ui.approvalKeyboard(props.sessionId, MAIN_BRANCH, view.value?.approvals ?? []))
const composer = ref<InstanceType<typeof ComposerPanel>>()
const contextUsage = ref<ContextUsage>()
const branchable = computed(() => connection.supports("session.branch_end") && view.value?.status === "ready")
const canNewLine = computed(() => connection.supports("session.branch_create") && view.value?.status === "ready")
watch(() => view.value?.status, (status) => { if (status === "ready") void models.load() }, { immediate: true })
const { commands, failure } = useSessionCommands({
  sessionId: () => props.sessionId,
  view,
  summary,
  contextUsage,
  confirm: (message, options) => forkConfirm.value?.open(message, options) ?? Promise.resolve(false),
})
/** Asks the main line to commit the changes of the turn that just ended. */
async function commit(): Promise<void> {
  try {
    await chat.submit(props.sessionId, MAIN_BRANCH, t("timeline.commitPrompt"))
  } catch (cause) {
    failure(cause)
  }
}
const sessionCommands = computed(() => commands.value ? { ...commands.value, tasks: () => panes.show(props.sessionId, "tasks") } : undefined)

// Text a panel puts into this Session's composer (the review comments of the changes panel).
watch(() => ui.composerInsert, (request) => {
  if (request === undefined || request.sessionId !== props.sessionId) return
  ui.composerInsert = undefined
  if (composer.value !== undefined) {
    void composer.value.insert(request.text)
    return
  }
  const draft = drafts.get(`session:${props.sessionId}:main`)
  draft.text = draft.text === "" ? request.text : `${draft.text}\n\n${request.text}`
})

// A shortcut opens a menu of the composer in the pane the operator is in.
watch(() => ui.composerMenu, (request) => {
  if (request === undefined || request.pane !== (props.pane ?? "main")) return
  ui.composerMenu = undefined
  composer.value?.openMenu(request.menu)
})

const listSkills = computed(() => {
  const cwd = summary.value?.cwd
  if (cwd === undefined || !connection.supports("skills.catalog")) return undefined
  return async () => (await connection.client.call("skills.catalog", { cwd }, readSkills)).entries.filter((skill) => skill.enabled && skill.shadowedBy === undefined)
})
watch(() => props.sessionId, () => { contextUsage.value = undefined })
watch(() => [props.sessionId, view.value?.status, view.value?.running] as const, async ([sessionId, status, running]) => {
  if (status !== "ready" || running || !view.value?.methods.includes("session.context.usage")) return
  try {
    const reading = await chat.usage(sessionId)
    if (sessionId === props.sessionId) contextUsage.value = reading
  } catch { /* the meter keeps its last reading; /usage reports the failure */ }
}, { immediate: true })

function send(text: string, attachments: DraftAttachments): Promise<void> {
  return chat.submit(props.sessionId, MAIN_BRANCH, text, attachments)
}

async function setPlanning(active: boolean): Promise<void> {
  try {
    await chat.setPlanning(props.sessionId, active)
  } catch (cause) {
    failure(cause)
  }
}

function stop(): Promise<void> {
  return chat.stop(props.sessionId, MAIN_BRANCH)
}

function selectModel(model: ModelId): Promise<void> {
  return chat.setModel(props.sessionId, model)
}

function openLine(branchId: string): void {
  chat.ensureLine(props.sessionId, branchId)
  panes.openLine(props.sessionId, branchId)
}

function branch(text: string, cut: number): void {
  ui.requestNewLine({ sessionId: props.sessionId, parentBranchId: MAIN_BRANCH, at: cut, text })
}

async function createBranch(prompt: string): Promise<void> {
  const at = view.value?.lastTurnEnd
  if (at === undefined) throw new Error(t("lines.noFinishedTurn"))
  const branchId = await chat.createLine(props.sessionId, MAIN_BRANCH, at, prompt)
  panes.openLine(props.sessionId, branchId)
}

/** Makes the split pane's Session the routed one, and the routed one the split. */
async function swapPanes(): Promise<void> {
  const main = route.params.sessionId
  if (typeof main !== "string") return
  await router.push(`/chat/${props.sessionId}`)
  ui.openSplit(main)
  ui.activePane = "main"
}

async function setPermission(preset: PermissionPreset): Promise<void> {
  try {
    const outcome = await chat.setPermission(props.sessionId, preset)
    if (outcome === "conflict") toast.push({ tone: "info", title: t("permission.conflict") })
  } catch (cause) {
    failure(cause)
  }
}
</script>

<template>
  <section v-if="view?.status === 'closed'" class="removed-state route-page">
    <h1 data-route-heading tabindex="-1">{{ t("chat.removedTitle") }}</h1>
    <p>{{ t("chat.removedDescription") }}</p>
    <RouterLink to="/chat/new">{{ t("sidebar.newSession") }}</RouterLink>
  </section>

  <section v-else-if="view?.status === 'failed'" class="removed-state route-page">
    <h1 data-route-heading tabindex="-1">{{ title }}</h1>
    <p role="alert">{{ view.error }}</p>
  </section>

  <section v-else-if="!view || view.status === 'loading'" class="chat-loading" aria-busy="true">
    <h1 class="sr-only">{{ t("route.session") }}</h1>
    <BrandMark animated class="boot-mark" />
    <p>{{ t("common.loading") }}</p>
  </section>

  <section v-else class="chat-page">
    <ConfirmDialog ref="forkConfirm" />
    <TurnChangesDialog
      v-if="fileChanges"
      ref="changesDialog"
      :cwd="summary?.cwd"
      :load="(turnId) => chat.turnChanges(sessionId, MAIN_BRANCH, turnId)"
      :rewind="(turnId, paths) => chat.rewindTurn(sessionId, MAIN_BRANCH, turnId, paths)"
    />
    <div class="chat-page-top">
      <header class="session-header" data-tauri-drag-region="deep">
        <button v-if="ui.shellMode === 'compact'" class="ui-icon-button session-nav" data-variant="ghost" data-size="md" type="button" :aria-label="t('sidebar.openMenu')" @click="ui.drawerOpen = true"><Menu aria-hidden="true" /></button>
        <div class="session-heading">
          <SessionCrumb v-if="summary" :session-id="sessionId" :cwd="summary.cwd" :interactive="!split" />
          <SessionTitleMenu v-if="!split && summary" :session-id="sessionId" :title="title" />
          <h1 v-else data-route-heading tabindex="-1" :title="title">{{ title }}</h1>
        </div>
        <div class="session-tools">
          <SessionOutline v-if="outline.length > 1" :markers="outline" @jump="timeline?.jumpTo($event)" />
          <SessionPaneButtons v-if="!split" :session-id="sessionId" />
          <OpenInMenu v-if="!split && summary" :path="summary.cwd" />
          <TranscriptViewMenu v-if="!split" :session-id="sessionId" />
          <template v-if="split">
            <button type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('split.swap')" v-tooltip="t('split.swap')" @click="swapPanes"><ArrowLeftRight aria-hidden="true" /></button>
            <button type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('split.close')" v-tooltip="t('split.close')" @click="ui.closeSplit"><X aria-hidden="true" /></button>
          </template>
        </div>
        <span v-if="view.sessionRunning" class="ui-badge session-running" data-tone="primary"><span class="status-dot" data-state="running" aria-hidden="true" />{{ t(view.stopping ? "status.stopping" : "status.running") }}</span>
      </header>
    </div>
    <TimelineView
      ref="timeline"
      :rows="view.rows"
      :running="view.running"
      :session-id="sessionId"
      :quotable="!approval && questions.length === 0 && reviews.length === 0"
      :branchable="branchable && !approval && questions.length === 0 && reviews.length === 0"
      :file-changes="fileChanges"
      :next-steps="commands?.review !== undefined"
      @changes="changesDialog?.open($event)"
      @review="commands?.review('')"
      @commit="commit"
      @branch="branch"
      @open-line="openLine"
      @quote="composer?.quote($event)"
    />
    <div v-if="view.running && view.queued.length" class="composer-panel">
      <QueuedInputs :session-id="sessionId" :items="view.queued" />
    </div>
    <div v-if="view.todos.length" class="composer-panel">
      <TodoPanel :todos="view.todos" />
    </div>
    <div v-if="approval" class="composer-panel">
      <ApprovalPrompt :key="JSON.stringify([approval.branchId, approval.requestId])" :session-id="sessionId" :request="approval" :keyboard="approvalKeyboard" :rules-available="rulesAvailable" />
    </div>
    <div v-else-if="questions.length" class="composer-panel question-prompts">
      <QuestionPrompt v-for="question in questions" :key="JSON.stringify([question.branchId, question.requestId, question.callId])" :session-id="sessionId" :question="question" />
    </div>
    <div v-else-if="reviews.length" class="composer-panel question-prompts">
      <PlanReview v-for="review in reviews" :key="JSON.stringify([review.branchId, review.requestId, review.callId])" :session-id="sessionId" :review="review" />
    </div>
    <ComposerPanel
      v-else
      ref="composer"
      :draft-key="`session:${sessionId}:main`"
      :running="view.running"
      :stopping="view.stopping"
      :model="view.model"
      :send="send"
      :stores-pastes="storesPastes"
      :search-files="searchFiles"
      :permission="permissionControl"
      :stop="stop"
      :model-selectable="connection.supports('session.model.update')"
      :select-model="selectModel"
      :reasoning-effort="view.reasoningEffort"
      :select-effort="connection.supports('session.model.update') ? async (value) => { await chat.setEffort(sessionId, value) } : undefined"
      :create-branch="canNewLine ? createBranch : undefined"
      :open-lines="branchable ? () => panes.openLines(sessionId) : undefined"
      :session-commands="sessionCommands"
      :list-skills="listSkills"
      :context-usage="contextUsage"
      :show-context="() => panes.show(sessionId, 'details')"
      :plan-mode="planControl"
      :history="sentMessages"
    />
  </section>
</template>

<style scoped>
.session-running { gap: 6px; }
.session-running > .status-dot { border: 0; }
.session-tools { display: flex; flex: none; align-items: center; gap: var(--space-1); }
</style>
