<script setup lang="ts">
import { computed } from "vue"
import { RouterLink } from "vue-router"
import { useI18n } from "vue-i18n"
import { Archive, Columns2, Copy, Pencil, Pin, PinOff, Trash2 } from "lucide-vue-next"
import { useConnectionStore } from "../../stores/connection"
import { useGitStore } from "../../stores/git"
import { useUiStore } from "../../stores/ui"
import type { SessionSummary } from "../../rpc/protocol"
import RowActionMenu from "../../components/RowActionMenu.vue"
import { formatRelativeTimeShort } from "../session/transcript/timeline-time"
import { useSessionState } from "./session-state"

/** A Session in the sidebar: its state, title and age, and what can be done with it. */
const props = defineProps<{ session: SessionSummary; activeSessionId?: string; nowMs: number; busy?: boolean }>()
const emit = defineEmits<{ navigate: []; action: [kind: "rename" | "archive" | "delete", session: SessionSummary]; copy: [path: string] }>()
const { t } = useI18n()
const connection = useConnectionStore()
const ui = useUiStore()
const stateOf = useSessionState()
const state = computed(() => stateOf(props.session))
const name = computed(() => props.session.title ?? props.session.preview ?? t("sidebar.untitled"))
const pinned = computed(() => ui.pinnedSessions.includes(props.session.sessionId))
const git = useGitStore()
/** A Session in a linked worktree sits under its main repository, marked with the worktree's branch. */
const branch = computed(() => git.linkedMain(props.session.cwd) === undefined ? undefined : git.places[props.session.cwd]?.branch)
</script>

<template>
  <li class="session-row">
    <RouterLink
      class="session-open"
      :to="{ name: 'chat-session', params: { sessionId: session.sessionId } }"
      :data-open="session.sessionId === activeSessionId ? 'true' : undefined"
      :aria-current="session.sessionId === activeSessionId ? 'page' : undefined"
      :title="session.title ?? session.preview"
      @click="emit('navigate')"
    >
      <span class="status-dot" :data-state="state" aria-hidden="true" />
      <strong>{{ name }}<span v-if="branch" class="session-branch">{{ branch }}</span><span v-if="state !== 'idle'" class="sr-only">, {{ t(`sidebar.state.${state}`) }}</span></strong>
      <small>{{ formatRelativeTimeShort(new Date(session.updatedAt), nowMs) }}</small>
    </RouterLink>
    <span class="session-actions"><RowActionMenu :label="t('sidebar.sessionActions', { name })">
      <button type="button" class="ui-menu-item" @click="ui.togglePinned(session.sessionId)"><PinOff v-if="pinned" aria-hidden="true" /><Pin v-else aria-hidden="true" />{{ t(pinned ? "sidebar.unpin" : "sidebar.pin") }}</button>
      <button v-if="connection.supports('session.methods')" type="button" class="ui-menu-item" :disabled="busy || connection.phase !== 'connected'" @click="emit('action', 'rename', session)"><Pencil aria-hidden="true" />{{ t("sidebar.rename") }}</button>
      <button v-if="connection.supports('session.archive')" type="button" class="ui-menu-item" :disabled="busy || session.running || connection.phase !== 'connected'" @click="emit('action', 'archive', session)"><Archive aria-hidden="true" />{{ t("sidebar.archive") }}</button>
      <button v-if="ui.splitAvailable && activeSessionId !== undefined && session.sessionId !== activeSessionId" type="button" class="ui-menu-item" @click="ui.openSplit(session.sessionId)"><Columns2 aria-hidden="true" />{{ t("split.open") }}</button>
      <button type="button" class="ui-menu-item" @click="emit('copy', session.cwd)"><Copy aria-hidden="true" />{{ t("workspace.copyPath") }}</button>
      <button v-if="connection.supports('session.delete')" type="button" class="ui-menu-item" :disabled="busy || session.running || connection.phase !== 'connected'" @click="emit('action', 'delete', session)"><Trash2 aria-hidden="true" />{{ t("sidebar.delete") }}</button>
    </RowActionMenu></span>
  </li>
</template>

<style scoped>
/* The session link reuses the row geometry of the kit's .session-open. */
.session-list .session-open { grid-template-columns: var(--control-xs) minmax(0, 1fr) auto; border-radius: var(--control-sm-radius); color: inherit; text-decoration: none; }
.session-row { display: flex; align-items: center; gap: 2px; }
.session-row .session-open { flex: 1; min-width: 0; }
.session-branch { margin-left: 6px; color: var(--muted-foreground); font-family: var(--font-mono); font-size: var(--text-2xs); font-weight: var(--weight-regular); }
.session-actions { flex: none; opacity: .4; }
.session-row:is(:hover, :focus-within) .session-actions { opacity: 1; }
.session-list .session-open .status-dot { justify-self: center; }
.session-list .session-open:hover { background: var(--hover); }
.session-list .session-open[data-open="true"] { background: var(--card); box-shadow: var(--shadow-xs), 0 0 0 1px var(--border); }
@media (hover: none) {
  .session-actions { opacity: 1; }
}
</style>
