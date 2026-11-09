<script setup lang="ts">
import { computed, type Component } from "vue"
import { useI18n } from "vue-i18n"
import { Activity, AppWindow, FileDiff, Files, GitBranch, List, ListChecks, SquareTerminal } from "lucide-vue-next"
import { withKeys, type ShortcutCommand } from "../../../app/shortcuts"
import { MAIN_BRANCH } from "../../../rpc/protocol"
import { useChatStore } from "../../../stores/chat"
import { useConnectionStore } from "../../../stores/connection"
import { useFilesStore } from "../../../stores/files"
import { usePanesStore, type DetailView } from "../../../stores/panes"
import { useSessionsStore } from "../../../stores/sessions"
import { changedFileCount } from "../../panes/changes/session-changes"

type PaneButton = { view: DetailView; icon: Component; label: string; command?: ShortcutCommand; count?: number; tone?: "primary" | "warning" }

/**
 * The session header's toggles for the right panel's views, as Claude Code's: an icon each, its
 * name and shortcut in the tooltip and a count where the view has one; pressed while it shows.
 */
const props = defineProps<{ sessionId: string }>()
const { t } = useI18n()
const chat = useChatStore()
const connection = useConnectionStore()
const files = useFilesStore()
const panes = usePanesStore()
const sessions = useSessionsStore()

const view = computed(() => chat.views[props.sessionId])
const layout = computed(() => panes.of(props.sessionId))
const shown = (detail: DetailView) => layout.value.open && layout.value.view === detail

const buttons = computed<PaneButton[]>(() => {
  const changed = changedFileCount(view.value?.rows ?? [], sessions.byId.get(props.sessionId)?.cwd)
  const hasPlan = view.value?.planning === true || (view.value?.todos.length ?? 0) > 0 || view.value?.rows.some((row) => row.kind === "tool" && row.name === "exit_plan_mode") === true
  const tasks = view.value?.backgroundTasks.filter((task) => !task.finish).length ?? 0
  const lines = view.value?.lines.filter((line) => line.id !== MAIN_BRANCH) ?? []
  const linesBusy = lines.some((line) => line.state.kind === "working" || view.value?.approvals.some((request) => request.branchId === line.id))
  return [
    { view: "details", icon: List, label: t("details.title") },
    ...(changed > 0 || shown("changes") ? [{ view: "changes" as const, icon: FileDiff, label: t("sessionChanges.title"), command: "toggleDiff" as const, ...(changed > 0 ? { count: changed } : {}) }] : []),
    ...(files.readable(props.sessionId) ? [{ view: "files" as const, icon: Files, label: t("files.title"), command: "toggleFiles" as const }] : []),
    { view: "preview", icon: AppWindow, label: t("preview.title"), command: "togglePreview" },
    ...(connection.supports("terminal.open") ? [{ view: "terminal" as const, icon: SquareTerminal, label: t("terminal.title"), command: "toggleTerminal" as const }] : []),
    ...(hasPlan || shown("plan") ? [{ view: "plan" as const, icon: ListChecks, label: t("plan.panel") }] : []),
    { view: "tasks", icon: Activity, label: t("tasks.title"), ...(tasks > 0 ? { count: tasks, tone: "primary" as const } : {}) },
    ...(connection.supports("session.branch_end") ? [{ view: "lines" as const, icon: GitBranch, label: t("lines.title"), ...(lines.length > 0 ? { count: lines.length, ...(linesBusy ? { tone: "warning" as const } : {}) } : {}) }] : []),
  ]
})

function toggle(button: PaneButton): void {
  if (button.view === "lines") panes.toggleLines(props.sessionId)
  else panes.toggle(props.sessionId, button.view)
}
</script>

<template>
  <div class="pane-buttons" role="group" :aria-label="t('detail.views')">
    <button
      v-for="button in buttons"
      :key="button.view"
      type="button"
      class="ui-button pane-button"
      data-variant="ghost"
      data-size="sm"
      :aria-pressed="shown(button.view)"
      :aria-label="button.label"
      v-tooltip="button.command ? withKeys(button.label, button.command) : button.label"
      @click="toggle(button)"
    >
      <component :is="button.icon" aria-hidden="true" />
      <span v-if="button.count !== undefined" class="pane-count" :data-tone="button.tone">{{ button.count }}</span>
    </button>
  </div>
</template>

<style scoped>
.pane-buttons { display: flex; flex: none; align-items: center; gap: 2px; }
.pane-button { min-width: var(--control-sm); gap: 4px; padding-inline: 7px; color: var(--muted-foreground); }
.pane-button:hover, .pane-button[aria-pressed="true"] { color: var(--foreground); }
.pane-button[aria-pressed="true"] { background: var(--press); }
.pane-count { color: var(--fg-2); font-size: var(--text-xs); font-variant-numeric: tabular-nums; }
.pane-count[data-tone="primary"] { color: var(--primary-ink); }
.pane-count[data-tone="warning"] { color: var(--warning); }
</style>
