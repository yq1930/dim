<script setup lang="ts">
import { computed, watch } from "vue"
import { useI18n } from "vue-i18n"
import { ChevronLeft, LoaderCircle, Square } from "lucide-vue-next"
import { MAIN_BRANCH } from "../../../rpc/protocol"
import { useChatStore, type SessionMember } from "../../../stores/chat"
import TimelineView from "../../session/transcript/TimelineView.vue"
import { memberStatus, memberTask } from "./member-state"

/** One member's own Session, read-only: its transcript as it works, and a stop while it runs. */
const props = defineProps<{ member: SessionMember }>()
const emit = defineEmits<{ back: [] }>()
const { t } = useI18n()
const chat = useChatStore()

watch(() => props.member.sessionId, (sessionId) => { if (sessionId !== undefined) chat.open(sessionId) }, { immediate: true })
const view = computed(() => props.member.sessionId === undefined ? undefined : chat.views[props.member.sessionId])
const status = computed(() => memberStatus(props.member))

function stop(): void {
  if (props.member.sessionId !== undefined) void chat.stop(props.member.sessionId, MAIN_BRANCH)
}
</script>

<template>
  <section class="member-conversation" :aria-label="memberTask(member)">
    <header class="member-crumb">
      <button type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('members.all')" v-tooltip="t('members.all')" @click="emit('back')">
        <ChevronLeft aria-hidden="true" />
      </button>
      <strong>{{ memberTask(member) }}</strong>
      <span class="member-state" :data-status="status">{{ t(`members.status.${status}`) }}</span>
      <button v-if="status === 'running' && view?.running" type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('members.stop')" v-tooltip="t('members.stop')" @click="stop">
        <Square aria-hidden="true" />
      </button>
    </header>
    <div v-if="!view || view.status === 'loading'" class="member-loading" role="status">
      <LoaderCircle :size="16" class="is-spinning" aria-hidden="true" />
      <span>{{ view ? t("common.loading") : t("members.starting") }}</span>
    </div>
    <TimelineView
      v-else
      :rows="view.rows"
      :running="view.running"
      :run-started-at="view.turnStartedAt"
      :quotable="false"
      :empty="t('members.noActivity')"
      variant="panel"
      :session-id="member.sessionId"
    />
  </section>
</template>

<style scoped>
.member-conversation { display: flex; min-width: 0; min-height: 0; flex: 1; flex-direction: column; background: var(--background); }
.member-conversation > :deep(.timeline-shell) { min-height: 0; flex: 1 1 auto; }
.member-crumb { display: flex; min-width: 0; flex: none; align-items: center; gap: var(--space-2); padding: var(--space-2) var(--space-3); border-bottom: 1px solid var(--border); }
.member-crumb strong { min-width: 0; flex: 1; overflow: hidden; color: var(--foreground); font-size: var(--text-md); font-weight: var(--weight-medium); text-overflow: ellipsis; white-space: nowrap; }
.member-state { flex: none; color: var(--muted-foreground); font-size: var(--text-xs); }
.member-state[data-status="running"] { color: var(--primary); }
.member-state[data-status="failed"] { color: var(--danger); }
.member-loading { display: flex; min-height: 0; flex: 1; align-items: center; justify-content: center; gap: var(--space-2); color: var(--muted-foreground); font-size: var(--text-sm); }
</style>
