<script setup lang="ts">
import { computed, ref, watch } from "vue"
import { RouterLink, useRoute, useRouter } from "vue-router"
import { useI18n } from "vue-i18n"
import { Activity, ArrowLeftRight, FileDiff, GitBranch, ListChecks, Menu, SquareTerminal, X } from "lucide-vue-next"
import BrandMark from "../../components/BrandMark.vue"
import ConfirmDialog from "../../components/ConfirmDialog.vue"
import ApprovalPrompt from "./ApprovalPrompt.vue"
import ComposerPanel from "./ComposerPanel.vue"
import QueuedInputs from "./QueuedInputs.vue"
import PlanReview from "./PlanReview.vue"
import QuestionPrompt from "./QuestionPrompt.vue"
import TimelineView from "./TimelineView.vue"
import TodoPanel from "./TodoPanel.vue"
import TurnChangesDialog from "./TurnChangesDialog.vue"
import { useComposerDrafts } from "./composer-drafts"
import { useSessionCommands } from "./session-commands"
import { changedFileCount } from "./session-changes"
import { MAIN_BRANCH, type ContextUsage, type DraftAttachments, type ModelId, type PermissionPreset } from "../../rpc/protocol"
import { readSkills } from "../../rpc/general-settings"
import { useChatStore } from "../../stores/chat"
import { useConnectionStore } from "../../stores/connection"
import { useModelsStore } from "../../stores/models"
import { useSessionsStore, workspaceName } from "../../stores/sessions"
import { useToastStore } from "../../stores/toast"
import { useUiStore, type DetailView, type Pane } from "../../stores/ui"

/** A Session's transcript and composer; `pane` is "split" for the one shown beside the routed Session. */
const props = defineProps<{ sessionId: string; pane?: Pane }>()
const { t } = useI18n()
const models = useModelsStore()
const chat = useChatStore()
const connection = useConnectionStore()
const sessions = useSessionsStore()
const ui = useUiStore()
const toast = useToastStore()
const route = useRoute()
const router = useRouter()
const forkConfirm = ref<InstanceType<typeof ConfirmDialog>>()
const drafts = useComposerDrafts()

watch(() => props.sessionId, (id) => chat.open(id), { immediate: true })

const view = computed(() => chat.views[props.sessionId])
const summary = computed(() => sessions.byId.get(props.sessionId))
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
const changedFiles = computed(() => changedFileCount(view.value?.rows ?? [], summary.value?.cwd))
const split = computed(() => props.pane === "split")
const shown = (detail: DetailView) => ui.detailOpen && ui.detailView === detail
const hasPlan = computed(() => view.value?.planning === true || (view.value?.todos.length ?? 0) > 0 || view.value?.rows.some((row) => row.kind === "tool" && row.name === "exit_plan_mode") === true)
/** The main line's messages, oldest first, for the composer's ↑ recall. */
const sentMessages = computed(() => (view.value?.rows ?? []).flatMap((row) => row.kind === "user" && row.text.trim() !== "" ? [row.text] : []))
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
const backgroundTasks = computed(() => view.value?.backgroundTasks ?? [])
const activeTasks = computed(() => backgroundTasks.value.filter((task) => !task.finish).length)
const lineCount = computed(() => view.value?.lines.filter((line) => line.id !== MAIN_BRANCH).length ?? 0)
const linesBusy = computed(() => {
  const approvals = view.value?.approvals ?? []
  return view.value?.lines.some((line) => line.id !== MAIN_BRANCH && (line.state.kind === "working" || approvals.some((request) => request.branchId === line.id))) ?? false
})
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
const sessionCommands = computed(() => commands.value ? { ...commands.value, tasks: () => ui.openDetail("tasks") } : undefined)

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
  ui.openLine(props.sessionId, branchId)
}

function branch(text: string, cut: number): void {
  ui.requestNewLine({ sessionId: props.sessionId, parentBranchId: MAIN_BRANCH, at: cut, text })
}

async function createBranch(prompt: string): Promise<void> {
  const at = view.value?.lastTurnEnd
  if (at === undefined) throw new Error(t("lines.noFinishedTurn"))
  const branchId = await chat.createLine(props.sessionId, MAIN_BRANCH, at, prompt)
  ui.openLine(props.sessionId, branchId)
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
      <header class="session-header">
        <button v-if="ui.shellMode === 'compact'" class="ui-icon-button session-nav" data-variant="ghost" data-size="md" type="button" :aria-label="t('sidebar.openMenu')" @click="ui.drawerOpen = true"><Menu aria-hidden="true" /></button>
        <div class="session-heading">
          <template v-if="summary">
            <span class="workspace-crumb" :title="summary.cwd">{{ workspaceName(summary.cwd) }}</span>
            <span class="workspace-crumb-separator" aria-hidden="true">/</span>
          </template>
          <h1 data-route-heading tabindex="-1" :title="title">{{ title }}</h1>
        </div>
        <button
          v-if="!split && (changedFiles > 0 || shown('changes'))"
          class="ui-button session-lines"
          data-variant="ghost"
          data-size="sm"
          type="button"
          :aria-pressed="shown('changes')"
          :aria-label="t('sessionChanges.open')"
          v-tooltip="t('sessionChanges.open')"
          @click="ui.toggleDetail('changes')"
        >
          <FileDiff aria-hidden="true" />
          <span>{{ t("sessionChanges.title") }}</span>
          <span v-if="changedFiles > 0" class="ui-badge" data-size="sm">{{ changedFiles }}</span>
        </button>
        <button v-if="!split && (hasPlan || shown('plan'))" type="button" class="ui-button session-lines" data-variant="ghost" data-size="sm" :aria-pressed="shown('plan')" :aria-label="t('plan.panel')" v-tooltip="t('plan.panel')" @click="ui.toggleDetail('plan')">
          <ListChecks aria-hidden="true" /><span>{{ t("plan.panel") }}</span>
        </button>
        <button
          v-if="!split && connection.supports('session.branch_end')"
          class="ui-button session-lines"
          data-variant="ghost"
          data-size="sm"
          type="button"
          :aria-label="t('lines.open')"
          @click="ui.toggleLines(sessionId)"
        >
          <GitBranch aria-hidden="true" />
          <span>{{ t("lines.title") }}</span>
          <span v-if="lineCount > 0" class="ui-badge" data-size="sm" :data-tone="linesBusy ? 'warning' : undefined">{{ lineCount }}</span>
        </button>
        <button v-if="!split" type="button" class="ui-button session-lines" data-variant="ghost" data-size="sm" :aria-pressed="shown('tasks')" :aria-label="t('tasks.title')" v-tooltip="t('tasks.title')" @click="ui.toggleDetail('tasks')">
          <Activity aria-hidden="true" /><span>{{ t("tasks.title") }}</span><span v-if="activeTasks" class="ui-badge" data-tone="primary">{{ activeTasks }}</span>
        </button>
        <button v-if="!split && connection.supports('terminal.open')" type="button" class="ui-button session-lines" data-variant="ghost" data-size="sm" :aria-pressed="shown('terminal')" :aria-label="t('terminal.title')" v-tooltip="t('terminal.toggle')" @click="ui.toggleDetail('terminal')">
          <SquareTerminal aria-hidden="true" /><span>{{ t("terminal.title") }}</span>
        </button>
        <template v-if="split">
          <button type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('split.swap')" v-tooltip="t('split.swap')" @click="swapPanes"><ArrowLeftRight aria-hidden="true" /></button>
          <button type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('split.close')" v-tooltip="t('split.close')" @click="ui.closeSplit"><X aria-hidden="true" /></button>
        </template>
        <span v-if="view.sessionRunning" class="ui-badge session-running" data-tone="primary"><span class="status-dot" data-state="running" aria-hidden="true" />{{ t(view.stopping ? "status.stopping" : "status.running") }}</span>
      </header>
    </div>
    <TimelineView
      :rows="view.rows"
      :running="view.running"
      :quotable="!approval && questions.length === 0 && reviews.length === 0"
      :branchable="branchable && !approval && questions.length === 0 && reviews.length === 0"
      :file-changes="fileChanges"
      @changes="changesDialog?.open($event)"
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
      :open-lines="branchable ? () => ui.openLines(sessionId) : undefined"
      :session-commands="sessionCommands"
      :list-skills="listSkills"
      :context-usage="contextUsage"
      :plan-mode="planControl"
      :history="sentMessages"
    />
  </section>
</template>

<style scoped>
.session-running { gap: 6px; }
.session-running > .status-dot { border: 0; }
.session-lines { gap: 6px; flex: none; }
.session-lines[aria-pressed="true"] { background: var(--hover); color: var(--foreground); }
.session-lines .ui-badge { min-width: 18px; padding-inline: 4px; }
</style>
