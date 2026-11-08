<script setup lang="ts">
import { computed, ref } from "vue"
import { RouterLink } from "vue-router"
import { useI18n } from "vue-i18n"
import { ChevronRight, Copy, Folder, FolderOpen, Loader2, Plus, X } from "lucide-vue-next"
import { useSessionsStore, type Workspace } from "../../stores/sessions"
import type { SessionSummary } from "../../rpc/protocol"
import RowActionMenu from "../../components/RowActionMenu.vue"
import SidebarSessionRow from "./SidebarSessionRow.vue"
import { useSessionState } from "./session-state"

/** One workspace (a working directory) and its sessions, most recent first; `hasPinned` marks one whose other sessions sit pinned above. */
const props = defineProps<{ workspace: Workspace; expanded: boolean; activeSessionId?: string; nowMs: number; busy?: boolean; hasPinned?: boolean }>()
const emit = defineEmits<{ toggle: []; navigate: []; action: [kind: "rename" | "archive" | "delete", session: SessionSummary]; copy: [path: string] }>()
const { t } = useI18n()
const sessions = useSessionsStore()
const stateOf = useSessionState()

// A group shows five sessions until "show more" walks out twenty at a time.
const FIRST = 5
const STEP = 20
const limit = ref(FIRST)
const shown = computed(() => props.workspace.sessions.slice(0, limit.value))
const hasMore = computed(() => props.workspace.sessions.length > limit.value)
const running = computed(() => !props.expanded && props.workspace.sessions.some((session) => session.running))
const waiting = computed(() => !props.expanded && props.workspace.sessions.some((session) => stateOf(session) === "waiting"))

function toggleReveal(): void {
  limit.value = hasMore.value ? limit.value + STEP : FIRST
}
</script>

<template>
  <section class="workspace-group">
    <div class="workspace-row" @click="emit('toggle')">
      <button
        class="workspace-toggle"
        type="button"
        :aria-expanded="expanded"
        :aria-label="t(expanded ? 'sidebar.collapseWorkspace' : 'sidebar.expandWorkspace', { name: workspace.name })"
        @click.stop="emit('toggle')"
      >
        <span class="workspace-disclosure" aria-hidden="true">
          <span class="workspace-disclosure-chevron" :data-open="expanded ? 'true' : undefined"><ChevronRight :size="14" /></span>
          <span class="workspace-disclosure-icon">
            <FolderOpen v-if="expanded" :size="14" />
            <Folder v-else :size="14" />
          </span>
        </span>
        <strong :title="workspace.cwd">{{ workspace.name }}</strong>
      </button>
      <!-- A collapsed group with a run in flight shows a spinner where the actions fade in. -->
      <span v-if="waiting" class="workspace-running" aria-hidden="true"><span class="status-dot" data-state="waiting" /></span>
      <span v-else-if="running" class="workspace-running" aria-hidden="true"><Loader2 :size="14" class="is-spinning" /></span>
      <div class="workspace-row-actions" @click.stop>
        <RouterLink
          class="ui-icon-button workspace-add" data-variant="ghost" data-size="sm"
          :to="{ name: 'chat-new', query: { cwd: workspace.cwd } }"
          :aria-label="t('sidebar.newInWorkspace', { name: workspace.name })"
          v-tooltip="t('sidebar.newInWorkspace', { name: workspace.name })"
          @click="emit('navigate')"
        ><Plus aria-hidden="true" /></RouterLink>
        <RowActionMenu :label="t('workspace.actions', { name: workspace.name })">
          <button type="button" class="ui-menu-item" @click="emit('copy', workspace.cwd)"><Copy aria-hidden="true" />{{ t("workspace.copyPath") }}</button>
          <button v-if="!workspace.sessions.length" type="button" class="ui-menu-item" @click="sessions.forgetWorkspace(workspace.cwd)"><X aria-hidden="true" />{{ t("workspace.removeRecent") }}</button>
        </RowActionMenu>
      </div>
    </div>
    <ul v-if="expanded" class="session-list">
      <SidebarSessionRow
        v-for="session in shown"
        :key="session.sessionId"
        :session="session"
        :active-session-id="activeSessionId"
        :now-ms="nowMs"
        :busy="busy"
        @navigate="emit('navigate')"
        @action="(kind, target) => emit('action', kind, target)"
        @copy="emit('copy', $event)"
      />
      <li v-if="workspace.sessions.length > FIRST">
        <button class="ui-button reveal-more" data-variant="ghost" data-size="sm" type="button" @click="toggleReveal">
          <ChevronRight :data-open="hasMore ? undefined : 'true'" aria-hidden="true" />
          {{ t(hasMore ? "sidebar.showMore" : "sidebar.showLess") }}
        </button>
      </li>
    </ul>
    <p v-if="expanded && !workspace.sessions.length && !hasPinned" class="sidebar-empty">{{ t("workspace.empty") }}</p>
  </section>
</template>

<style scoped>
.workspace-row { position: relative; cursor: pointer; }
/* The disclosure is a fixed 14px slot where the folder icon cross-fades into a chevron. */
.workspace-disclosure { position: relative; display: block; width: 14px; height: 14px; flex: none; }
.workspace-disclosure-chevron,
.workspace-disclosure-icon { position: absolute; inset: 0; display: grid; place-items: center; color: var(--muted-foreground); transition: opacity var(--duration-hover) ease; }
.workspace-disclosure-chevron { opacity: 0; }
.workspace-disclosure-chevron > svg { transition: transform var(--duration-press) var(--ease-out); }
.workspace-disclosure-chevron[data-open="true"] > svg { transform: rotate(90deg); }
.workspace-row:is(:hover, :focus-within) .workspace-disclosure-chevron { opacity: 1; }
.workspace-row:is(:hover, :focus-within) .workspace-disclosure-icon { opacity: 0; }
.workspace-row-actions { display: flex; align-items: center; gap: 1px; opacity: 0; transition: opacity var(--duration-hover) ease; }
.workspace-row:is(:hover, :focus-within) .workspace-row-actions { opacity: 1; }
.workspace-running { position: absolute; top: 0; right: 0; bottom: 0; display: grid; width: var(--control-sm); place-items: center; color: var(--muted-foreground); pointer-events: none; }
.workspace-row:is(:hover, :focus-within) .workspace-running { opacity: 0; }
@media (hover: none) {
  .workspace-row-actions { opacity: 1; }
  .workspace-running { display: none; }
}
</style>
