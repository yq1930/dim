<script setup lang="ts">
import type { DiffLine } from "./tool-display"

/**
 * A display diff: removed lines on red, added on green, context plain. `numbered` shows each
 * line's number in its file; `pickable` lets a line be picked (a review comment), `marked`
 * flags the picked ones, and the `after` slot renders under a line.
 */
defineProps<{ lines: readonly DiffLine[]; numbered?: boolean; pickable?: boolean; marked?: ReadonlySet<number> }>()
const emit = defineEmits<{ pick: [index: number] }>()
</script>

<template>
  <div class="diff-lines" :data-numbered="numbered ? '' : undefined">
    <template v-for="(line, index) in lines" :key="index">
      <div
        class="diff-line"
        :data-sign="line.sign"
        :data-marked="marked?.has(index) ? '' : undefined"
        :role="pickable ? 'button' : undefined"
        :tabindex="pickable ? 0 : undefined"
        @click="pickable && emit('pick', index)"
        @keydown.enter.prevent="pickable && emit('pick', index)"
      ><span v-if="numbered" class="diff-number" aria-hidden="true">{{ line.line ?? "" }}</span><span class="diff-sign" aria-hidden="true">{{ line.sign }}</span>{{ line.text }}</div>
      <slot name="after" :index="index" />
    </template>
  </div>
</template>

<style scoped>
.diff-lines { padding: var(--space-2) 0; font: var(--text-sm)/var(--leading-mono) var(--font-mono); }
.diff-line { padding: 0 var(--space-3); white-space: pre-wrap; overflow-wrap: anywhere; }
.diff-line[data-sign="-"] { background: color-mix(in oklab, var(--danger) 8%, transparent); color: color-mix(in oklab, var(--danger) 72%, var(--foreground)); }
.diff-line[data-sign="+"] { background: color-mix(in oklab, var(--success) 10%, transparent); color: color-mix(in oklab, var(--success) 66%, var(--foreground)); }
.diff-line[role="button"] { cursor: pointer; }
.diff-line[role="button"]:hover, .diff-line[role="button"]:focus-visible { box-shadow: inset 2px 0 0 var(--primary); outline: none; }
.diff-line[data-marked] { box-shadow: inset 3px 0 0 var(--warning); }
.diff-number { display: inline-block; min-width: 4ch; margin-right: var(--space-2); color: color-mix(in oklab, var(--muted-foreground) 70%, transparent); text-align: right; user-select: none; }
.diff-sign { display: inline-block; width: 2ch; user-select: none; }
</style>
