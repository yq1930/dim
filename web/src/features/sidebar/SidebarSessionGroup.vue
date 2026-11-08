<script setup lang="ts">
import { computed, ref } from "vue"
import { useI18n } from "vue-i18n"
import { ChevronRight } from "lucide-vue-next"
import type { SessionSummary } from "../../rpc/protocol"
import { workspaceName } from "../../stores/sessions"
import SidebarSessionRow from "./SidebarSessionRow.vue"

/** A headed run of Sessions from any workspace, each row naming its workspace; `label` is absent when ungrouped. */
const props = defineProps<{ label?: string; sessions: SessionSummary[]; activeSessionId?: string; nowMs: number; busy?: boolean }>()
const emit = defineEmits<{ navigate: []; action: [kind: "rename" | "archive" | "delete", session: SessionSummary]; copy: [path: string] }>()
const { t } = useI18n()

// A group shows ten Sessions until "show more" walks out thirty at a time.
const FIRST = 10
const STEP = 30
const limit = ref(FIRST)
const shown = computed(() => props.sessions.slice(0, limit.value))
const hasMore = computed(() => props.sessions.length > limit.value)
</script>

<template>
  <section class="session-group">
    <p v-if="label" class="session-group-label">{{ label }}</p>
    <ul class="session-list flat-sessions">
      <SidebarSessionRow
        v-for="session in shown"
        :key="session.sessionId"
        :session="session"
        :workspace="workspaceName(session.cwd)"
        :active-session-id="activeSessionId"
        :now-ms="nowMs"
        :busy="busy"
        @navigate="emit('navigate')"
        @action="(kind, target) => emit('action', kind, target)"
        @copy="emit('copy', $event)"
      />
      <li v-if="sessions.length > FIRST">
        <button class="ui-button reveal-more" data-variant="ghost" data-size="sm" type="button" @click="limit = hasMore ? limit + STEP : FIRST">
          <ChevronRight :data-open="hasMore ? undefined : 'true'" aria-hidden="true" />
          {{ t(hasMore ? "sidebar.showMore" : "sidebar.showLess") }}
        </button>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.session-group + .session-group { margin-top: var(--space-2); }
.session-group-label { margin: var(--space-1) var(--space-2); color: var(--muted-foreground); font-size: var(--text-xs); font-weight: var(--weight-medium); }
.flat-sessions { padding-left: 0; }
</style>
