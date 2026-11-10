<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue"
import { RouterLink, useRoute, useRouter } from "vue-router"
import { useI18n } from "vue-i18n"
import { Archive, CalendarClock, FolderOpen, Keyboard, PanelLeftClose, Plus, Search, Settings } from "lucide-vue-next"
import { useConnectionStore } from "../../stores/connection"
import { settingsMessages } from "../../i18n/settings"
import BrandMark from "../../components/BrandMark.vue"
import ConnectionBadge from "./ConnectionBadge.vue"
import SidebarWorkspaceGroup from "./SidebarWorkspaceGroup.vue"
import SidebarSessionRow from "./SidebarSessionRow.vue"
import { useSessionsStore } from "../../stores/sessions"
import { useUiStore } from "../../stores/ui"
import type { SessionSummary } from "../../rpc/protocol"
import { withKeys } from "../../app/shortcuts"
import WorkspaceOpenDialog from "./WorkspaceOpenDialog.vue"
import SessionRenameDialog from "./SessionRenameDialog.vue"
import ConfirmDialog from "../../components/ConfirmDialog.vue"
import SidebarSessionGroup from "./SidebarSessionGroup.vue"
import SidebarViewMenu from "./SidebarViewMenu.vue"
import { useSessionActions } from "./session-actions"
import { useSessionState } from "./session-state"
import { groupByDate, groupByState, sortSessions } from "./sidebar-list"

const emit = defineEmits<{ navigate: [] }>()
const { t } = useI18n()
const { t: settingsText } = useI18n({ messages: settingsMessages })
const route = useRoute()
const router = useRouter()
const sessions = useSessionsStore()
const connection = useConnectionStore()
const ui = useUiStore()
const search = ref("")
const workspaceDialog = ref<InstanceType<typeof WorkspaceOpenDialog>>()
const renameDialog = ref<InstanceType<typeof SessionRenameDialog>>()
const confirm = ref<InstanceType<typeof ConfirmDialog>>()
const actions = useSessionActions()
const busy = actions.busy
const stateOf = useSessionState()

async function openWorkspace(cwd: string): Promise<void> {
  sessions.rememberWorkspace(cwd)
  await router.push({ name: "chat-new", query: { cwd } })
  emit("navigate")
}
function copyPath(path: string): Promise<void> {
  return actions.copy(path, "chat.pathCopied")
}
async function action(kind: "rename" | "archive" | "delete", session: SessionSummary): Promise<void> {
  if (busy.value) return
  if (kind === "rename") { await renameDialog.value?.open(session); return }
  if (kind === "delete" && !await confirm.value?.open(t("sidebar.deleteConfirm", { name: session.title ?? session.preview ?? t("sidebar.untitled") }), { confirmLabel: t("sidebar.delete") })) return
  await actions.dispose(kind, session)
}
// Relative times re-render each minute without a reload.
const nowMs = ref(Date.now())
let clock: ReturnType<typeof setInterval> | undefined
onMounted(() => { clock = setInterval(() => { nowMs.value = Date.now() }, 60_000) })
onBeforeUnmount(() => clearInterval(clock))

const activeSessionId = computed(() => typeof route.params.sessionId === "string" ? route.params.sessionId : undefined)
const activeCwd = computed(() => {
  const cwd = activeSessionId.value === undefined ? undefined : sessions.byId.get(activeSessionId.value)?.cwd
  return cwd !== undefined && sessions.isScratch(cwd) ? "" : cwd
})
/** A current session seeds the new one's workspace. */
const newSessionTo = computed(() => ({ name: "chat-new" as const, ...(activeCwd.value === undefined ? {} : { query: { cwd: activeCwd.value } }) }))

const query = computed(() => search.value.trim().toLocaleLowerCase())
const titled = (session: SessionSummary) => [session.title, session.preview].some((value) => value?.toLocaleLowerCase().includes(query.value))
/** Pinned Sessions head the sidebar instead of sitting in their workspace. */
const pinned = computed(() => ui.pinnedSessions
  .map((sessionId) => sessions.byId.get(sessionId))
  .filter((session): session is SessionSummary => session !== undefined && (query.value === "" || titled(session) || session.cwd.toLocaleLowerCase().includes(query.value))))
const pinnedCwds = computed(() => new Set(ui.pinnedSessions.flatMap((sessionId) => sessions.byId.get(sessionId)?.cwd ?? [])))
/** The filter matches the workspace name or path, or the session preview. */
const visible = computed(() => {
  const groups = sessions.workspaces.flatMap((workspace) => {
    const unpinned = sortSessions(workspace.sessions.filter((session) => !ui.pinnedSessions.includes(session.sessionId)), ui.sidebarSortBy)
    if (query.value === "") return [{ ...workspace, sessions: unpinned }]
    const matched = workspace.name.toLocaleLowerCase().includes(query.value) || workspace.cwd.toLocaleLowerCase().includes(query.value)
    const matching = matched ? unpinned : unpinned.filter(titled)
    return !matched && matching.length === 0 ? [] : [{ ...workspace, sessions: matching }]
  })
  return ui.sidebarSortBy === "alpha" ? [...groups].sort((left, right) => left.name.localeCompare(right.name)) : groups
})
/** The unpinned Sessions of every workspace in one list, grouped by day or state, or not at all. */
const flat = computed(() => {
  const all = visible.value.flatMap((workspace) => workspace.sessions)
  if (ui.sidebarGroupBy === "date") return groupByDate(all, ui.sidebarSortBy, new Date(nowMs.value)).map((group) => ({ key: group.key, label: t(group.label), sessions: group.sessions }))
  if (ui.sidebarGroupBy === "state") return groupByState(all, ui.sidebarSortBy, stateOf).map((group) => ({ key: group.key, label: t(group.label), sessions: group.sessions }))
  return [{ key: "all", label: undefined, sessions: sortSessions(all, ui.sidebarSortBy) }]
})
</script>

<template>
  <aside class="app-sidebar" :aria-label="t('a11y.workspaceSessionIndex')">
    <header class="brand-row" data-tauri-drag-region="deep">
      <BrandMark class="brand-logo" />
      <strong class="brand-copy">{{ t("brand.product") }}</strong>
      <button class="ui-icon-button sidebar-collapse" data-variant="ghost" data-size="sm" type="button" :aria-label="t('sidebar.closeMenu')" v-tooltip="withKeys(t('sidebar.closeMenu'), 'toggleSidebar')" @click="ui.sidebarCollapsed = true">
        <PanelLeftClose aria-hidden="true" />
      </button>
    </header>

    <nav class="primary-actions" :aria-label="t('a11y.primaryNavigation')">
      <RouterLink class="ui-button sidebar-new" data-variant="secondary" data-size="md" :to="newSessionTo" @click="$emit('navigate')">
        <Plus aria-hidden="true" /><span class="sidebar-new-label">{{ t("sidebar.newSession") }}</span>
      </RouterLink>
      <button type="button" class="ui-button sidebar-new" data-variant="ghost" data-size="md" @click="workspaceDialog?.open(activeCwd ?? '')"><FolderOpen aria-hidden="true" /><span class="sidebar-new-label">{{ t("workspace.open") }}</span></button>
      <RouterLink v-if="connection.supports('schedule.list')" class="ui-button sidebar-new" data-variant="ghost" data-size="md" to="/schedules" @click="$emit('navigate')"><CalendarClock aria-hidden="true" /><span class="sidebar-new-label">{{ t("schedules.title") }}</span></RouterLink>
    </nav>

    <div class="ui-field-wrap sidebar-search">
      <Search aria-hidden="true" />
      <input v-model="search" class="ui-field" data-size="md" type="search" :placeholder="t('sidebar.searchPlaceholder')" :aria-label="t('common.search')">
    </div>

    <div class="workspace-scroll app-scroll-viewport">
      <template v-if="pinned.length">
        <p class="section-label">{{ t("sidebar.pinned") }}</p>
        <ul class="session-list pinned-sessions">
          <SidebarSessionRow
            v-for="session in pinned"
            :key="session.sessionId"
            :session="session"
            :active-session-id="activeSessionId"
            :now-ms="nowMs"
            :busy="busy"
            @navigate="$emit('navigate')"
            @action="action"
            @copy="copyPath"
          />
        </ul>
      </template>
      <div class="section-head">
        <p class="section-label">{{ t("sidebar.sessions") }}</p>
        <SidebarViewMenu />
      </div>
      <p v-if="sessions.error && !sessions.loaded" class="sidebar-empty" role="alert">{{ sessions.error }}</p>
      <p v-else-if="sessions.loaded && visible.length === 0" class="sidebar-empty">{{ t(query ? "sidebar.noMatches" : "sidebar.noWorkspaces") }}</p>
      <template v-if="ui.sidebarGroupBy !== 'project'">
        <SidebarSessionGroup
          v-for="group in flat"
          :key="group.key"
          :label="group.label"
          :sessions="group.sessions"
          :active-session-id="activeSessionId"
          :now-ms="nowMs"
          :busy="busy"
          @navigate="$emit('navigate')"
          @action="action"
          @copy="copyPath"
        />
      </template>
      <SidebarWorkspaceGroup
        v-for="workspace in ui.sidebarGroupBy === 'project' ? visible : []"
        :key="workspace.cwd"
        :workspace="workspace"
        :expanded="query !== '' || ui.isWorkspaceExpanded(workspace.cwd)"
        :active-session-id="activeSessionId"
        :now-ms="nowMs"
        :busy="busy"
        :has-pinned="pinnedCwds.has(workspace.cwd)"
        @toggle="ui.toggleWorkspace(workspace.cwd)"
        @navigate="$emit('navigate')"
        @action="action"
        @copy="copyPath"
      />
    </div>

    <footer class="sidebar-footer">
      <ConnectionBadge />
      <RouterLink v-if="connection.supports('session.archive')" class="ui-icon-button" data-variant="ghost" data-size="sm" to="/settings/archived" :aria-label="settingsText('archived')" v-tooltip="settingsText('archived')" @click="$emit('navigate')">
        <Archive aria-hidden="true" />
      </RouterLink>
      <button type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('shortcuts.title')" v-tooltip="withKeys(t('shortcuts.title'), 'showShortcuts')" @click="ui.shortcutsOpen = true">
        <Keyboard aria-hidden="true" />
      </button>
      <RouterLink class="ui-icon-button" data-variant="ghost" data-size="sm" to="/settings" :aria-label="settingsText('settings')" v-tooltip="withKeys(settingsText('settings'), 'openSettings')" @click="$emit('navigate')">
        <Settings aria-hidden="true" />
      </RouterLink>
    </footer>
  </aside>
  <WorkspaceOpenDialog ref="workspaceDialog" @select="openWorkspace" />
  <SessionRenameDialog ref="renameDialog" />
  <ConfirmDialog ref="confirm" />
</template>

<style scoped>
.sidebar-new { width: 100%; justify-content: flex-start; }
.sidebar-new-label { flex: 1; text-align: left; }
.section-head { display: flex; align-items: center; justify-content: space-between; padding-right: var(--space-1); }
.section-head .section-label { margin-block: var(--space-1); }
.sidebar-footer { display: flex; align-items: center; gap: 2px; }
.sidebar-footer > .connection-badge { min-width: 0; flex: 1; }
</style>
