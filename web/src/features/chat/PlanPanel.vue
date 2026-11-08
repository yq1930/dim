<script setup lang="ts">
import { computed } from "vue"
import { useI18n } from "vue-i18n"
import { ListChecks } from "lucide-vue-next"
import { useChatStore } from "../../stores/chat"
import ChatMarkdown from "./ChatMarkdown.vue"
import TodoPanel from "./TodoPanel.vue"

/**
 * The detail panel's plan of a Session's main line: the plan its latest
 * `exit_plan_mode` call submitted, with the operator's decision on it, and
 * the task list Planning keeps.
 */
const props = defineProps<{ sessionId: string }>()
const { t } = useI18n()
const chat = useChatStore()
const view = computed(() => chat.views[props.sessionId])
const todos = computed(() => view.value?.todos ?? [])
const plan = computed(() => {
  const rows = view.value?.rows ?? []
  for (let index = rows.length - 1; index >= 0; index -= 1) {
    const row = rows[index]
    if (row?.kind !== "tool" || row.name !== "exit_plan_mode" || typeof row.input.plan !== "string") continue
    const state = row.result === undefined ? "awaiting" : row.result.approved === true ? "approved" : "revised"
    return { key: row.key, text: row.input.plan, state }
  }
  return undefined
})
</script>

<template>
  <div class="plan-panel app-scroll-viewport">
    <p v-if="!plan && todos.length === 0" class="panel-empty">{{ t(view?.planning ? "plan.planning" : "plan.empty") }}</p>
    <article v-if="plan" class="plan-document">
      <header class="plan-document-header">
        <ListChecks :size="14" aria-hidden="true" />
        <strong>{{ t("plan.panel") }}</strong>
        <span class="ui-badge" data-size="sm" :data-tone="plan.state === 'approved' ? 'success' : plan.state === 'awaiting' ? 'warning' : undefined">{{ t(`plan.state.${plan.state}`) }}</span>
      </header>
      <ChatMarkdown :content="plan.text" :final="true" :message-id="`plan-panel-${plan.key}`" />
    </article>
    <TodoPanel v-if="todos.length" :todos="todos" />
  </div>
</template>

<style scoped>
.plan-panel { display: flex; min-width: 0; min-height: 0; flex-direction: column; gap: var(--space-3); padding: var(--space-3); overflow-y: auto; }
.plan-document { flex: none; padding: var(--space-3) var(--space-4); border: 1px solid var(--border); border-radius: var(--radius-lg); background: var(--card); }
.plan-document-header { display: flex; align-items: center; gap: var(--space-2); margin-bottom: var(--space-2); font-size: var(--text-sm); }
.plan-document-header > svg { flex: none; color: var(--muted-foreground); }
.plan-document-header > .ui-badge { margin-left: auto; }
</style>
