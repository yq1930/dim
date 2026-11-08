<script setup lang="ts">
import { computed } from "vue"
import { useI18n } from "vue-i18n"
import { Activity, FileDiff, GitBranch, ListChecks, SquareTerminal } from "lucide-vue-next"
import { withKeys } from "../../../app/shortcuts"
import { MAIN_BRANCH } from "../../../rpc/protocol"
import { useChatStore } from "../../../stores/chat"
import { useConnectionStore } from "../../../stores/connection"
import { usePanesStore, type DetailView } from "../../../stores/panes"
import { useSessionsStore } from "../../../stores/sessions"
import { changedFileCount } from "../../panes/changes/session-changes"

/** The session header's buttons for the right panel's views, pressed while their view shows. */
const props = defineProps<{ sessionId: string }>()
const { t } = useI18n()
const chat = useChatStore()
const connection = useConnectionStore()
const panes = usePanesStore()
const sessions = useSessionsStore()

const view = computed(() => chat.views[props.sessionId])
const layout = computed(() => panes.of(props.sessionId))
const shown = (detail: DetailView) => layout.value.open && layout.value.view === detail
const changedFiles = computed(() => changedFileCount(view.value?.rows ?? [], sessions.byId.get(props.sessionId)?.cwd))
const hasPlan = computed(() => view.value?.planning === true || (view.value?.todos.length ?? 0) > 0 || view.value?.rows.some((row) => row.kind === "tool" && row.name === "exit_plan_mode") === true)
const activeTasks = computed(() => view.value?.backgroundTasks.filter((task) => !task.finish).length ?? 0)
const lines = computed(() => view.value?.lines.filter((line) => line.id !== MAIN_BRANCH) ?? [])
const linesBusy = computed(() => lines.value.some((line) => line.state.kind === "working" || view.value?.approvals.some((request) => request.branchId === line.id)))
</script>

<template>
  <button
    v-if="changedFiles > 0 || shown('changes')"
    class="ui-button session-lines"
    data-variant="ghost"
    data-size="sm"
    type="button"
    :aria-pressed="shown('changes')"
    :aria-label="t('sessionChanges.open')"
    v-tooltip="withKeys(t('sessionChanges.open'), 'toggleDiff')"
    @click="panes.toggle(sessionId, 'changes')"
  >
    <FileDiff aria-hidden="true" />
    <span>{{ t("sessionChanges.title") }}</span>
    <span v-if="changedFiles > 0" class="ui-badge" data-size="sm">{{ changedFiles }}</span>
  </button>
  <button v-if="hasPlan || shown('plan')" type="button" class="ui-button session-lines" data-variant="ghost" data-size="sm" :aria-pressed="shown('plan')" :aria-label="t('plan.panel')" v-tooltip="t('plan.panel')" @click="panes.toggle(sessionId, 'plan')">
    <ListChecks aria-hidden="true" /><span>{{ t("plan.panel") }}</span>
  </button>
  <button v-if="connection.supports('session.branch_end')" class="ui-button session-lines" data-variant="ghost" data-size="sm" type="button" :aria-label="t('lines.open')" @click="panes.toggleLines(sessionId)">
    <GitBranch aria-hidden="true" />
    <span>{{ t("lines.title") }}</span>
    <span v-if="lines.length > 0" class="ui-badge" data-size="sm" :data-tone="linesBusy ? 'warning' : undefined">{{ lines.length }}</span>
  </button>
  <button type="button" class="ui-button session-lines" data-variant="ghost" data-size="sm" :aria-pressed="shown('tasks')" :aria-label="t('tasks.title')" v-tooltip="t('tasks.title')" @click="panes.toggle(sessionId, 'tasks')">
    <Activity aria-hidden="true" /><span>{{ t("tasks.title") }}</span><span v-if="activeTasks" class="ui-badge" data-tone="primary">{{ activeTasks }}</span>
  </button>
  <button v-if="connection.supports('terminal.open')" type="button" class="ui-button session-lines" data-variant="ghost" data-size="sm" :aria-pressed="shown('terminal')" :aria-label="t('terminal.title')" v-tooltip="withKeys(t('terminal.toggle'), 'toggleTerminal')" @click="panes.toggle(sessionId, 'terminal')">
    <SquareTerminal aria-hidden="true" /><span>{{ t("terminal.title") }}</span>
  </button>
</template>

<style scoped>
.session-lines { gap: 6px; flex: none; }
.session-lines[aria-pressed="true"] { background: var(--hover); color: var(--foreground); }
.session-lines .ui-badge { min-width: 18px; padding-inline: 4px; }
</style>
