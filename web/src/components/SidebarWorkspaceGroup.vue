<script setup lang="ts">
import { computed, ref } from "vue"
import { RouterLink } from "vue-router"
import { useI18n } from "vue-i18n"
import { Archive, ChevronRight, Copy, Folder, FolderOpen, Loader2, Pencil, Plus, Trash2, X } from "lucide-vue-next"
import { useSessionsStore, type Workspace } from "../stores/sessions"
import { useConnectionStore } from "../stores/connection"
import type { SessionSummary } from "../rpc/protocol"
import RowActionMenu from "./RowActionMenu.vue"
import { formatRelativeTimeShort } from "../features/chat/timeline-time"

/** One workspace (a working directory) and its sessions, most recent first. */
const props = defineProps<{ workspace: Workspace; expanded: boolean; activeSessionId?: string; nowMs: number; busy?: boolean }>()
const emit = defineEmits<{ toggle: []; navigate: []; action: [kind: "rename" | "archive" | "delete", session: SessionSummary]; copy: [path: string] }>()
const { t } = useI18n()
const sessions = useSessionsStore()
const connection = useConnectionStore()

// A group shows five sessions until "show more" walks out twenty at a time.
const FIRST = 5
const STEP = 20
const limit = ref(FIRST)
const shown = computed(() => props.workspace.sessions.slice(0, limit.value))
const hasMore = computed(() => props.workspace.sessions.length > limit.value)
const running = computed(() => !props.expanded && props.workspace.sessions.some((session) => session.running))

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
      <span v-if="running" class="workspace-running" aria-hidden="true"><Loader2 :size="14" class="is-spinning" /></span>
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
      <li v-for="session in shown" :key="session.sessionId" class="session-row">
        <RouterLink
          class="session-open"
          :to="{ name: 'chat-session', params: { sessionId: session.sessionId } }"
          :data-open="session.sessionId === activeSessionId ? 'true' : undefined"
          :aria-current="session.sessionId === activeSessionId ? 'page' : undefined"
          :title="session.title ?? session.preview"
          @click="emit('navigate')"
        >
          <span class="status-dot" :data-state="session.running ? 'running' : 'idle'" aria-hidden="true" />
          <strong>{{ session.title ?? session.preview ?? t("sidebar.untitled") }}</strong>
          <small>{{ formatRelativeTimeShort(new Date(session.updatedAt), nowMs) }}</small>
        </RouterLink>
        <span class="session-actions"><RowActionMenu :label="t('sidebar.sessionActions', { name: session.title ?? session.preview ?? t('sidebar.untitled') })">
          <button v-if="connection.supports('session.methods')" type="button" class="ui-menu-item" :disabled="busy || connection.phase !== 'connected'" @click="emit('action', 'rename', session)"><Pencil aria-hidden="true" />{{ t("sidebar.rename") }}</button>
          <button v-if="connection.supports('session.archive')" type="button" class="ui-menu-item" :disabled="busy || session.running || connection.phase !== 'connected'" @click="emit('action', 'archive', session)"><Archive aria-hidden="true" />{{ t("sidebar.archive") }}</button>
          <button type="button" class="ui-menu-item" @click="emit('copy', session.cwd)"><Copy aria-hidden="true" />{{ t("workspace.copyPath") }}</button>
          <button v-if="connection.supports('session.delete')" type="button" class="ui-menu-item" :disabled="busy || session.running || connection.phase !== 'connected'" @click="emit('action', 'delete', session)"><Trash2 aria-hidden="true" />{{ t("sidebar.delete") }}</button>
        </RowActionMenu></span>
      </li>
      <li v-if="workspace.sessions.length > FIRST">
        <button class="ui-button reveal-more" data-variant="ghost" data-size="sm" type="button" @click="toggleReveal">
          <ChevronRight :data-open="hasMore ? undefined : 'true'" aria-hidden="true" />
          {{ t(hasMore ? "sidebar.showMore" : "sidebar.showLess") }}
        </button>
      </li>
    </ul>
    <p v-if="expanded && !workspace.sessions.length" class="sidebar-empty">{{ t("workspace.empty") }}</p>
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
/* The session link reuses the row geometry of the kit's .session-open. */
.session-list .session-open { grid-template-columns: var(--control-xs) minmax(0, 1fr) auto; border-radius: var(--control-sm-radius); color: inherit; text-decoration: none; }
.session-row { display: flex; align-items: center; gap: 2px; }
.session-row .session-open { flex: 1; min-width: 0; }
.session-actions { flex: none; opacity: .4; }
.session-row:is(:hover, :focus-within) .session-actions { opacity: 1; }
.session-list .session-open .status-dot { justify-self: center; }
.session-list .session-open:hover { background: var(--hover); }
.session-list .session-open[data-open="true"] { background: var(--card); box-shadow: var(--shadow-xs), 0 0 0 1px var(--border); }
@media (hover: none) {
  .workspace-row-actions { opacity: 1; }
  .workspace-running { display: none; }
  .session-actions { opacity: 1; }
}
</style>
