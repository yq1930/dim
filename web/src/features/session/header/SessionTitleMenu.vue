<script setup lang="ts">
import { computed, ref } from "vue"
import { useI18n } from "vue-i18n"
import { Archive, ChevronDown, Copy, Hash, Pencil, Pin, PinOff, Trash2 } from "lucide-vue-next"
import ConfirmDialog from "../../../components/ConfirmDialog.vue"
import RowActionMenu from "../../../components/RowActionMenu.vue"
import { useConnectionStore } from "../../../stores/connection"
import { useSessionsStore } from "../../../stores/sessions"
import { useUiStore } from "../../../stores/ui"
import SessionRenameDialog from "../../sidebar/SessionRenameDialog.vue"
import { useSessionActions } from "../../sidebar/session-actions"

/** The session header's title, as Claude Code's: it opens what can be done with the Session. */
const props = defineProps<{ sessionId: string; title: string }>()
const { t } = useI18n()
const connection = useConnectionStore()
const sessions = useSessionsStore()
const ui = useUiStore()
const actions = useSessionActions()
const renameDialog = ref<InstanceType<typeof SessionRenameDialog>>()
const confirm = ref<InstanceType<typeof ConfirmDialog>>()
const session = computed(() => sessions.byId.get(props.sessionId))
const pinned = computed(() => ui.pinnedSessions.includes(props.sessionId))
const offline = computed(() => actions.busy.value || connection.phase !== "connected")

async function remove(): Promise<void> {
  const target = session.value
  if (target === undefined || !await confirm.value?.open(t("sidebar.deleteConfirm", { name: props.title }), { confirmLabel: t("sidebar.delete") })) return
  await actions.dispose("delete", target)
}
</script>

<template>
  <RowActionMenu :label="t('sidebar.sessionActions', { name: title })">
    <template #trigger="{ open, expanded }">
      <h1 data-route-heading tabindex="-1" class="session-title">
        <button type="button" class="session-title-button" aria-haspopup="dialog" :aria-expanded="expanded" :title="title" @click.stop="open($event.currentTarget as HTMLElement)">
          <span>{{ title }}</span><ChevronDown aria-hidden="true" />
        </button>
      </h1>
    </template>
    <button v-if="session && connection.supports('session.methods')" type="button" class="ui-menu-item" :disabled="offline" @click="renameDialog?.open(session)"><Pencil aria-hidden="true" />{{ t("sidebar.rename") }}</button>
    <button type="button" class="ui-menu-item" @click="ui.togglePinned(sessionId)"><PinOff v-if="pinned" aria-hidden="true" /><Pin v-else aria-hidden="true" />{{ t(pinned ? "sidebar.unpin" : "sidebar.pin") }}</button>
    <button v-if="session" type="button" class="ui-menu-item" @click="actions.copy(session.cwd, 'chat.pathCopied')"><Copy aria-hidden="true" />{{ t("workspace.copyPath") }}</button>
    <button type="button" class="ui-menu-item" @click="actions.copy(sessionId, 'session.idCopied')"><Hash aria-hidden="true" />{{ t("session.copyId") }}</button>
    <template v-if="session && (connection.supports('session.archive') || connection.supports('session.delete'))">
      <hr class="ui-menu-separator">
      <button v-if="connection.supports('session.archive')" type="button" class="ui-menu-item" :disabled="offline || session.running" @click="actions.dispose('archive', session)"><Archive aria-hidden="true" />{{ t("sidebar.archive") }}</button>
      <button v-if="connection.supports('session.delete')" type="button" class="ui-menu-item" data-tone="danger" :disabled="offline || session.running" @click="remove"><Trash2 aria-hidden="true" />{{ t("sidebar.delete") }}</button>
    </template>
  </RowActionMenu>
  <SessionRenameDialog ref="renameDialog" />
  <ConfirmDialog ref="confirm" />
</template>

<style scoped>
.session-title { display: flex; min-width: 0; margin: 0; }
.session-title:focus-visible { outline: none; }
.session-title-button { display: inline-flex; min-width: 0; flex: 0 1 auto; height: var(--control-sm); align-items: center; gap: var(--space-1); padding: 0 var(--space-2); margin-left: calc(-1 * var(--space-2)); border: 0; border-radius: var(--control-sm-radius); background: transparent; color: var(--foreground); font-size: var(--text-md); font-weight: var(--weight-medium); cursor: pointer; }
.session-title-button > span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.session-title-button > svg { width: 14px; height: 14px; flex: none; color: var(--muted-foreground); transition: transform var(--duration-press) var(--ease-out); }
.session-title-button[aria-expanded="true"] > svg { transform: rotate(180deg); }
.session-title-button:hover, .session-title-button[aria-expanded="true"] { background: var(--hover); }
</style>
