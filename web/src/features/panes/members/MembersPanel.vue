<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue"
import { useI18n } from "vue-i18n"
import { Ban, CircleCheck, CircleX, LoaderCircle } from "lucide-vue-next"
import { useChatStore, type SessionMember } from "../../../stores/chat"
import { usePanesStore } from "../../../stores/panes"
import { formatElapsedSeconds } from "../../session/transcript/timeline-time"
import MemberConversation from "./MemberConversation.vue"
import { memberStatus, memberTask } from "./member-state"

/** The work a Session handed to team members: unfinished first, each opening the member's own transcript. */
const props = defineProps<{ sessionId: string }>()
const { t } = useI18n()
const chat = useChatStore()
const panes = usePanesStore()

const members = computed(() => chat.views[props.sessionId]?.members ?? [])
const selected = computed(() => members.value.find((member) => member.requestId === panes.activeMember[props.sessionId]))
const now = ref(Date.now())
let ticker: ReturnType<typeof setInterval> | undefined
watch(() => members.value.some((member) => member.finish === undefined), (running) => {
  clearInterval(ticker)
  now.value = Date.now()
  if (running) ticker = setInterval(() => { now.value = Date.now() }, 1000)
}, { immediate: true })
onBeforeUnmount(() => clearInterval(ticker))

function elapsed(member: SessionMember): string {
  return formatElapsedSeconds((member.finish?.at ?? now.value) - member.startedAt)
}
</script>

<template>
  <MemberConversation v-if="selected" :key="selected.requestId" :member="selected" @back="panes.openMember(sessionId, undefined)" />
  <ul v-else-if="members.length" class="member-list" :aria-label="t('members.title')">
    <li v-for="member in members" :key="member.requestId">
      <button type="button" class="member-row" @click="panes.openMember(sessionId, member.requestId)">
        <LoaderCircle v-if="memberStatus(member) === 'running'" :size="14" class="is-spinning member-icon" data-status="running" aria-hidden="true" />
        <CircleCheck v-else-if="memberStatus(member) === 'completed'" :size="14" class="member-icon" aria-hidden="true" />
        <CircleX v-else-if="memberStatus(member) === 'failed'" :size="14" class="member-icon" data-status="failed" aria-hidden="true" />
        <Ban v-else :size="14" class="member-icon" aria-hidden="true" />
        <span class="member-task">{{ memberTask(member) || t("members.member") }}</span>
        <span class="member-meta">{{ t(`members.status.${memberStatus(member)}`) }} · {{ elapsed(member) }}</span>
      </button>
    </li>
  </ul>
  <p v-else class="members-empty">{{ t("members.empty") }}</p>
</template>

<style scoped>
.member-list { display: flex; min-height: 0; flex: 1; flex-direction: column; gap: 2px; margin: 0; padding: var(--space-2); overflow-y: auto; list-style: none; }
.member-row { display: flex; width: 100%; min-width: 0; align-items: center; gap: var(--space-2); padding: var(--space-2); border: 0; border-radius: var(--radius-md); background: transparent; color: var(--foreground); font: inherit; font-size: var(--text-sm); text-align: left; cursor: pointer; }
.member-row:hover { background: var(--hover); }
.member-icon { flex: none; color: var(--muted-foreground); }
.member-icon[data-status="running"] { color: var(--primary); }
.member-icon[data-status="failed"] { color: var(--danger); }
.member-task { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.member-meta { flex: none; color: var(--muted-foreground); font-size: var(--text-xs); font-variant-numeric: tabular-nums; }
.members-empty { margin: 0; padding: var(--space-6) var(--space-4); color: var(--muted-foreground); font-size: var(--text-sm); text-align: center; }
</style>
