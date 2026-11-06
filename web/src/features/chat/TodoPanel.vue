<script setup lang="ts">
import { computed, ref } from "vue"
import { useI18n } from "vue-i18n"
import { ChevronDown, Circle, CircleCheck, LoaderCircle } from "lucide-vue-next"
import type { TodoItem } from "../../rpc/protocol"

/** The main line's task list above the composer; it folds to its header once every task is done. */
const props = defineProps<{ todos: TodoItem[] }>()
const { t } = useI18n()
const done = computed(() => props.todos.filter((todo) => todo.status === "completed").length)
const chosen = ref<boolean>()
const expanded = computed(() => chosen.value ?? done.value < props.todos.length)
</script>

<template>
  <section class="todo-panel" :aria-label="t('todo.title')">
    <button type="button" class="todo-header" :aria-expanded="expanded" @click="chosen = !expanded">
      <span class="todo-title">{{ t("todo.title") }}</span>
      <span class="todo-count">{{ done }}/{{ todos.length }}</span>
      <ChevronDown class="todo-chevron" :data-open="expanded ? '' : undefined" :size="14" aria-hidden="true" />
    </button>
    <ol v-if="expanded" class="todo-list">
      <li v-for="(todo, index) in todos" :key="index" class="todo-item" :data-status="todo.status">
        <CircleCheck v-if="todo.status === 'completed'" :size="14" aria-hidden="true" />
        <LoaderCircle v-else-if="todo.status === 'in_progress'" :size="14" aria-hidden="true" />
        <Circle v-else :size="14" aria-hidden="true" />
        <span>{{ todo.content }}</span>
      </li>
    </ol>
  </section>
</template>

<style scoped>
.todo-panel { border: 1px solid var(--border); border-radius: var(--radius-lg); background: var(--card); font-size: var(--text-sm); }
.todo-header { display: flex; align-items: center; gap: var(--space-2); width: 100%; padding: var(--space-2) var(--space-3); border: 0; background: none; color: inherit; font: inherit; cursor: pointer; }
.todo-title { font-weight: var(--weight-medium); }
.todo-count { color: var(--muted-foreground); font-variant-numeric: tabular-nums; }
.todo-chevron { margin-left: auto; color: var(--muted-foreground); transition: transform var(--duration-fast, 120ms) ease; }
.todo-chevron[data-open] { transform: rotate(180deg); }
.todo-list { display: grid; gap: var(--space-1); max-height: 30vh; margin: 0; padding: 0 var(--space-3) var(--space-3); overflow-y: auto; list-style: none; }
.todo-item { display: flex; align-items: flex-start; gap: var(--space-2); min-width: 0; line-height: 1.4; }
.todo-item > svg { flex: none; margin-top: 2px; color: var(--muted-foreground); }
.todo-item > span { min-width: 0; overflow-wrap: anywhere; }
.todo-item[data-status="in_progress"] { font-weight: var(--weight-medium); }
.todo-item[data-status="in_progress"] > svg { color: var(--accent-strong); }
.todo-item[data-status="completed"] > span { color: var(--muted-foreground); text-decoration: line-through; }
</style>
