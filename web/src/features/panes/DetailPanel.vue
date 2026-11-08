<script setup lang="ts">
import { computed, defineAsyncComponent, provide, useId, type Component } from "vue"
import { useI18n } from "vue-i18n"
import { Activity, AppWindow, ChevronDown, FileDiff, Files, GitBranch, GitPullRequest, ListChecks, Maximize2, Minimize2, SquareTerminal, X } from "lucide-vue-next"
import RowActionMenu from "../../components/RowActionMenu.vue"
import LinesPanel from "./lines/LinesPanel.vue"
import PlanPanel from "./plan/PlanPanel.vue"
import SessionChangesPanel from "./changes/SessionChangesPanel.vue"
import TasksPanel from "./tasks/TasksPanel.vue"
import GitPanel from "./git/GitPanel.vue"
import { changedFileCount } from "./changes/session-changes"
import { LINK_CONTEXT } from "../../components/link-context"
import { MAIN_BRANCH } from "../../rpc/protocol"
import { useChatStore } from "../../stores/chat"
import { useConnectionStore } from "../../stores/connection"
import { useFilesStore } from "../../stores/files"
import { useGitStore } from "../../stores/git"
import { useSessionsStore } from "../../stores/sessions"
import { useTerminalsStore } from "../../stores/terminals"
import { usePanesStore, type DetailView } from "../../stores/panes"
import { shortcutText, withKeys, type ShortcutCommand } from "../../app/shortcuts"

/** xterm.js loads only once a terminal is shown, and the file view's highlighter once a file is. */
const TerminalPanel = defineAsyncComponent(() => import("./terminal/TerminalPanel.vue"))
const FilesPanel = defineAsyncComponent(() => import("./files/FilesPanel.vue"))
const PreviewPanel = defineAsyncComponent(() => import("./preview/PreviewPanel.vue"))

type Tab = { view: DetailView; icon: Component; label: string; badge?: string | number; tone?: "primary" | "warning"; command?: ShortcutCommand }

/**
 * The right panel of the Session on screen, as a Claude Code pane: a header naming the view shown,
 * whose menu switches to another, with expand and close; the session header's buttons and the
 * shortcuts switch views too.
 */
const props = defineProps<{ sessionId: string }>()
const { t } = useI18n()
const chat = useChatStore()
const connection = useConnectionStore()
const files = useFilesStore()
const sessions = useSessionsStore()
const terminals = useTerminalsStore()
const git = useGitStore()
const panes = usePanesStore()
const id = useId()
provide(LINK_CONTEXT, computed(() => {
  const cwd = sessions.byId.get(props.sessionId)?.cwd
  return cwd === undefined ? undefined : { sessionId: props.sessionId, base: cwd, filePane: true }
}))

const tabs = computed<Tab[]>(() => {
  const view = chat.views[props.sessionId]
  const cwd = sessions.byId.get(props.sessionId)?.cwd
  const changed = changedFileCount(view?.rows ?? [], cwd)
  const todos = view?.todos ?? []
  const running = view?.backgroundTasks.filter((task) => !task.finish).length ?? 0
  const lines = view?.lines.filter((line) => line.id !== MAIN_BRANCH) ?? []
  const linesBusy = lines.some((line) => line.state.kind === "working" || view?.approvals.some((request) => request.branchId === line.id))
  return [
    { view: "changes", icon: FileDiff, label: t("sessionChanges.title"), command: "toggleDiff", ...(changed > 0 ? { badge: changed } : {}) },
    ...(files.readable(props.sessionId) ? [{ view: "files" as const, icon: Files, label: t("files.title"), command: "toggleFiles" as const }] : []),
    { view: "preview", icon: AppWindow, label: t("preview.title"), command: "togglePreview" },
    { view: "plan", icon: ListChecks, label: t("plan.panel"), ...(todos.length > 0 ? { badge: `${todos.filter((todo) => todo.status === "completed").length}/${todos.length}` } : {}) },
    { view: "tasks", icon: Activity, label: t("tasks.title"), ...(running > 0 ? { badge: running, tone: "primary" as const } : {}) },
    ...(terminals.available ? [{ view: "terminal" as const, icon: SquareTerminal, label: t("terminal.title"), command: "toggleTerminal" as const }] : []),
    ...(git.available && cwd !== undefined && git.places[cwd]?.root !== undefined ? [{ view: "git" as const, icon: GitPullRequest, label: t("git.tab") }] : []),
    ...(connection.supports("session.branch_end") ? [{ view: "lines" as const, icon: GitBranch, label: t("lines.title"), ...(lines.length > 0 ? { badge: lines.length, ...(linesBusy ? { tone: "warning" as const } : {}) } : {}) }] : []),
  ]
})
const layout = computed(() => panes.of(props.sessionId))
const current = computed<DetailView>(() => tabs.value.some((tab) => tab.view === layout.value.view) ? layout.value.view : "changes")
const shown = computed(() => tabs.value.find((tab) => tab.view === current.value) ?? tabs.value[0]!)
</script>

<template>
  <div class="detail-panel">
    <header class="panel-tabs" data-tauri-drag-region="deep">
      <RowActionMenu :label="t('detail.views')">
        <template #trigger="{ open, expanded }">
          <button type="button" class="pane-title" aria-haspopup="dialog" :aria-expanded="expanded" @click.stop="open($event.currentTarget as HTMLElement)">
            <component :is="shown.icon" :size="14" aria-hidden="true" />
            <span :id="`${id}-title`">{{ shown.label }}</span>
            <span v-if="shown.badge !== undefined" class="ui-badge" data-size="sm" :data-tone="shown.tone">{{ shown.badge }}</span>
            <ChevronDown class="pane-title-chevron" aria-hidden="true" />
          </button>
        </template>
        <button v-for="tab in tabs" :key="tab.view" type="button" role="menuitemradio" class="ui-menu-item" :aria-checked="tab.view === current" @click="panes.show(sessionId, tab.view)">
          <component :is="tab.icon" aria-hidden="true" />
          <span>{{ tab.label }}</span>
          <span v-if="tab.badge !== undefined" class="ui-badge" data-size="sm" :data-tone="tab.tone">{{ tab.badge }}</span>
          <kbd v-if="tab.command && shortcutText(tab.command)" class="ui-menu-meta">{{ shortcutText(tab.command) }}</kbd>
        </button>
      </RowActionMenu>
      <div class="panel-tab-actions">
        <button type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="layout.expanded ? t('lines.collapse') : t('lines.expand')" v-tooltip="withKeys(layout.expanded ? t('lines.collapse') : t('lines.expand'), 'toggleExpandPane')" @click="panes.toggleExpanded(sessionId)">
          <Minimize2 v-if="layout.expanded" aria-hidden="true" />
          <Maximize2 v-else aria-hidden="true" />
        </button>
        <button type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('detail.close')" v-tooltip="withKeys(t('detail.close'), 'closePane')" @click="panes.close(sessionId)">
          <X aria-hidden="true" />
        </button>
      </div>
    </header>
    <div class="detail-panel-body" role="region" :aria-labelledby="`${id}-title`">
      <SessionChangesPanel v-if="current === 'changes'" :session-id="sessionId" />
      <FilesPanel v-else-if="current === 'files'" :session-id="sessionId" />
      <PreviewPanel v-else-if="current === 'preview'" :session-id="sessionId" />
      <PlanPanel v-else-if="current === 'plan'" :session-id="sessionId" />
      <TasksPanel v-else-if="current === 'tasks'" :session-id="sessionId" />
      <TerminalPanel v-else-if="current === 'terminal'" :session-id="sessionId" />
      <GitPanel v-else-if="current === 'git'" :session-id="sessionId" />
      <LinesPanel v-else :session-id="sessionId" />
    </div>
  </div>
</template>

<style scoped>
.pane-title { display: flex; min-width: 0; height: var(--control-sm); align-items: center; gap: 6px; padding: 0 var(--space-2); border: 0; border-radius: var(--control-sm-radius); background: transparent; color: var(--foreground); font-size: var(--text-md); font-weight: var(--weight-medium); cursor: pointer; }
.pane-title > span:not(.ui-badge) { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.pane-title > svg { flex: none; }
.pane-title-chevron { width: 14px; height: 14px; color: var(--muted-foreground); transition: transform var(--duration-press) var(--ease-out); }
.pane-title[aria-expanded="true"] > .pane-title-chevron { transform: rotate(180deg); }
.pane-title:hover, .pane-title[aria-expanded="true"] { background: var(--hover); }
.panel-tabs > .panel-tab-actions { margin-left: auto; }
</style>
