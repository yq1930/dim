<script setup lang="ts">
import { computed, inject } from "vue"
import { ChevronRight } from "lucide-vue-next"
import { useI18n } from "vue-i18n"
import type { TranslationKey } from "../../../i18n"
import { formatElapsedSeconds, TIMELINE_NOW } from "./timeline-time"
import { thinkingPreview, thinkingTime, type ToolCategory, type WorkItem } from "./work"
import TimelineWorkItemView from "./TimelineWorkItem.vue"

/**
 * A work group reads as one summary line ("Thought for 12s · 3 reads, 1 command")
 * that expands into a left-railed list; a single item shows on its own. While one of
 * its thoughts is under way the line counts the group's thinking time and, folded,
 * shows the end of that thought.
 */
const props = defineProps<{
  items: WorkItem[]
  expanded: boolean
  itemExpanded: (item: WorkItem) => boolean
}>()
const emit = defineEmits<{ toggle: []; toggleItem: [item: WorkItem] }>()
const { t } = useI18n()
const now = inject(TIMELINE_NOW, undefined)

const CATEGORY_KEYS = {
  read: "work.read",
  edit: "work.edit",
  command: "work.command",
  search: "work.search",
  web_search: "work.webSearch",
  web_fetch: "work.webFetch",
  delegate: "work.delegate",
  unknown: "work.unknown",
} as const satisfies Record<ToolCategory, TranslationKey>
const CATEGORY_ORDER = Object.keys(CATEGORY_KEYS) as ToolCategory[]

const summary = computed(() => {
  let text = ""
  let previous: "thinking" | ToolCategory | undefined
  const thinking = thinkingTime(props.items, now?.value ?? 0)
  if (thinking.count > 0) {
    const duration = formatElapsedSeconds(thinking.running ? Math.floor(thinking.ms / 1000) * 1000 : thinking.ms)
    text = !thinking.timed ? (thinking.running ? t("work.thinking") : t("work.thinkingSteps", { count: thinking.count }, thinking.count))
      : t(thinking.running ? "work.thinkingFor" : "work.thoughtFor", { duration })
    previous = "thinking"
  }
  const counts = new Map<ToolCategory, number>()
  for (const item of props.items) {
    if (item.kind === "tool") counts.set(item.category, (counts.get(item.category) ?? 0) + 1)
  }
  for (const category of CATEGORY_ORDER) {
    const count = counts.get(category)
    if (count === undefined) continue
    if (previous !== undefined) text += t(previous === "thinking" ? "work.separator" : "work.actionSeparator")
    text += t(CATEGORY_KEYS[category], { count }, count)
    previous = category
  }
  return text
})
const running = computed(() => props.items.some((item) => item.kind === "thinking" ? item.running : item.status === "running"))
const preview = computed(() => {
  if (props.expanded) return ""
  for (let index = props.items.length - 1; index >= 0; index--) {
    const item = props.items[index]!
    if (item.kind === "thinking" && item.running) return thinkingPreview(item.content)
  }
  return ""
})
</script>

<template>
  <div class="timeline-work-row">
    <template v-if="items.length > 1">
      <button type="button" class="timeline-work-group-toggle" :class="{ 'is-running': running }" :aria-expanded="expanded" @click="emit('toggle')">
        <ChevronRight :size="14" aria-hidden="true" :class="{ 'is-expanded': expanded }" />
        <span>{{ summary }}</span>
        <small v-if="preview" class="timeline-thinking-preview" aria-hidden="true"><span>{{ preview }}</span></small>
      </button>
      <div v-if="expanded" class="timeline-work-group-content">
        <TimelineWorkItemView
          v-for="item in items"
          :key="item.id"
          :item="item"
          :expanded="itemExpanded(item)"
          @toggle="emit('toggleItem', item)"
        />
      </div>
    </template>
    <TimelineWorkItemView
      v-else-if="items[0]"
      :item="items[0]"
      :expanded="itemExpanded(items[0])"
      @toggle="emit('toggleItem', items[0])"
    />
  </div>
</template>
