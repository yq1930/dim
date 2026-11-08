<script setup lang="ts">
import { computed, defineAsyncComponent, nextTick, ref, useId, type Component } from "vue"
import { useI18n } from "vue-i18n"
import { Activity, FileDiff, GitBranch, ListChecks, Maximize2, Minimize2, SquareTerminal, X } from "lucide-vue-next"
import LinesPanel from "../features/lines/LinesPanel.vue"
import PlanPanel from "../features/chat/PlanPanel.vue"
import SessionChangesPanel from "../features/chat/SessionChangesPanel.vue"
import TasksPanel from "../features/chat/TasksPanel.vue"
import { changedFileCount } from "../features/chat/session-changes"
import { MAIN_BRANCH } from "../rpc/protocol"
import { useChatStore } from "../stores/chat"
import { useConnectionStore } from "../stores/connection"
import { useSessionsStore } from "../stores/sessions"
import { useTerminalsStore } from "../stores/terminals"
import { useUiStore, type DetailView } from "../stores/ui"

/** xterm.js loads only once a terminal is shown. */
const TerminalPanel = defineAsyncComponent(() => import("../features/terminal/TerminalPanel.vue"))

type Tab = { view: DetailView; icon: Component; label: string; badge?: string | number; tone?: "primary" | "warning" }

/** The right detail panel of the Session on screen: one tab per view of it. */
const props = defineProps<{ sessionId: string }>()
const { t } = useI18n()
const chat = useChatStore()
const connection = useConnectionStore()
const sessions = useSessionsStore()
const terminals = useTerminalsStore()
const ui = useUiStore()
const id = useId()
const tablist = ref<HTMLElement>()

const tabs = computed<Tab[]>(() => {
  const view = chat.views[props.sessionId]
  const changed = changedFileCount(view?.rows ?? [], sessions.byId.get(props.sessionId)?.cwd)
  const todos = view?.todos ?? []
  const running = view?.backgroundTasks.filter((task) => !task.finish).length ?? 0
  const lines = view?.lines.filter((line) => line.id !== MAIN_BRANCH) ?? []
  const linesBusy = lines.some((line) => line.state.kind === "working" || view?.approvals.some((request) => request.branchId === line.id))
  return [
    { view: "changes", icon: FileDiff, label: t("sessionChanges.title"), ...(changed > 0 ? { badge: changed } : {}) },
    { view: "plan", icon: ListChecks, label: t("plan.panel"), ...(todos.length > 0 ? { badge: `${todos.filter((todo) => todo.status === "completed").length}/${todos.length}` } : {}) },
    { view: "tasks", icon: Activity, label: t("tasks.title"), ...(running > 0 ? { badge: running, tone: "primary" as const } : {}) },
    ...(terminals.available ? [{ view: "terminal" as const, icon: SquareTerminal, label: t("terminal.title") }] : []),
    ...(connection.supports("session.branch_end") ? [{ view: "lines" as const, icon: GitBranch, label: t("lines.title"), ...(lines.length > 0 ? { badge: lines.length, ...(linesBusy ? { tone: "warning" as const } : {}) } : {}) }] : []),
  ]
})
const current = computed<DetailView>(() => tabs.value.some((tab) => tab.view === ui.detailView) ? ui.detailView : "changes")

function onKeydown(event: KeyboardEvent): void {
  if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return
  event.preventDefault()
  const index = tabs.value.findIndex((tab) => tab.view === current.value)
  const next = tabs.value[(index + (event.key === "ArrowRight" ? 1 : -1) + tabs.value.length) % tabs.value.length]
  if (next === undefined) return
  ui.openDetail(next.view)
  void nextTick(() => tablist.value?.querySelector<HTMLButtonElement>('[aria-selected="true"]')?.focus())
}
</script>

<template>
  <div class="detail-panel">
    <header class="panel-tabs" data-tauri-drag-region="deep">
      <div ref="tablist" class="panel-tablist" role="tablist" :aria-label="t('detail.views')" @keydown="onKeydown">
        <button
          v-for="tab in tabs"
          :id="`${id}-${tab.view}`"
          :key="tab.view"
          type="button"
          class="panel-tab"
          role="tab"
          :aria-selected="tab.view === current"
          :aria-controls="`${id}-body`"
          :tabindex="tab.view === current ? 0 : -1"
          @click="ui.openDetail(tab.view)"
        >
          <component :is="tab.icon" :size="14" aria-hidden="true" />
          <span>{{ tab.label }}</span>
          <span v-if="tab.badge !== undefined" class="ui-badge" data-size="sm" :data-tone="tab.tone">{{ tab.badge }}</span>
        </button>
      </div>
      <div class="panel-tab-actions">
        <button type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="ui.detailExpanded ? t('lines.collapse') : t('lines.expand')" v-tooltip="ui.detailExpanded ? t('lines.collapse') : t('lines.expand')" @click="ui.detailExpanded = !ui.detailExpanded">
          <Minimize2 v-if="ui.detailExpanded" aria-hidden="true" />
          <Maximize2 v-else aria-hidden="true" />
        </button>
        <button type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('detail.close')" v-tooltip="t('detail.close')" @click="ui.closeDetail">
          <X aria-hidden="true" />
        </button>
      </div>
    </header>
    <div :id="`${id}-body`" class="detail-panel-body" role="tabpanel" :aria-labelledby="`${id}-${current}`">
      <SessionChangesPanel v-if="current === 'changes'" :session-id="sessionId" />
      <PlanPanel v-else-if="current === 'plan'" :session-id="sessionId" />
      <TasksPanel v-else-if="current === 'tasks'" :session-id="sessionId" />
      <TerminalPanel v-else-if="current === 'terminal'" :session-id="sessionId" />
      <LinesPanel v-else :session-id="sessionId" />
    </div>
  </div>
</template>
