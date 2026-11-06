<script setup lang="ts">
import type { DiffLine } from "./tool-display"

/** A display diff: removed lines on red, added on green, context plain. */
defineProps<{ lines: readonly DiffLine[] }>()
</script>

<template>
  <div class="diff-lines">
    <div v-for="(line, index) in lines" :key="index" class="diff-line" :data-sign="line.sign"><span class="diff-sign" aria-hidden="true">{{ line.sign }}</span>{{ line.text }}</div>
  </div>
</template>

<style scoped>
.diff-lines { padding: var(--space-2) 0; font: var(--text-sm)/var(--leading-mono) var(--font-mono); }
.diff-line { padding: 0 var(--space-3); white-space: pre-wrap; overflow-wrap: anywhere; }
.diff-line[data-sign="-"] { background: color-mix(in oklab, var(--danger) 8%, transparent); color: color-mix(in oklab, var(--danger) 72%, var(--foreground)); }
.diff-line[data-sign="+"] { background: color-mix(in oklab, var(--success) 10%, transparent); color: color-mix(in oklab, var(--success) 66%, var(--foreground)); }
.diff-sign { display: inline-block; width: 2ch; user-select: none; }
</style>
