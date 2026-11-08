<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref } from "vue"
import { useI18n } from "vue-i18n"
import { TableOfContents } from "lucide-vue-next"

/** The Session's outline: one entry per message the operator sent, which scrolls the timeline to it. */
defineProps<{ markers: ReadonlyArray<{ key: string; index: number; prompt: string }> }>()
const emit = defineEmits<{ jump: [index: number] }>()
const { t } = useI18n()
const trigger = ref<HTMLButtonElement>()
const dialog = ref<HTMLDialogElement>()
const expanded = ref(false)
const left = ref(0)
const top = ref(0)

function close(): void { dialog.value?.close(); expanded.value = false }
async function open(): Promise<void> {
  const element = dialog.value
  if (!element || !trigger.value) return
  const anchor = trigger.value.getBoundingClientRect()
  left.value = Math.max(8, Math.min(anchor.right - 320, window.innerWidth - 328))
  top.value = anchor.bottom + 4
  element.showModal()
  expanded.value = true
  await nextTick()
  element.querySelector<HTMLButtonElement>("button")?.focus()
}
function key(event: KeyboardEvent): void {
  if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return
  event.preventDefault()
  const items = [...(dialog.value?.querySelectorAll<HTMLButtonElement>("button") ?? [])]
  const index = items.findIndex((item) => item === document.activeElement)
  const next = event.key === "Home" ? 0 : event.key === "End" ? items.length - 1 : (index + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length
  items[next]?.focus()
}
function choose(index: number): void {
  close()
  emit("jump", index)
}
onBeforeUnmount(close)
</script>

<template>
  <button ref="trigger" type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('outline.title')" aria-haspopup="dialog" :aria-expanded="expanded" v-tooltip="t('outline.title')" @click="open">
    <TableOfContents aria-hidden="true" />
  </button>
  <dialog ref="dialog" class="session-outline ui-popover" :style="{ left: `${left}px`, top: `${top}px` }" :aria-label="t('outline.title')" @cancel="expanded = false" @close="expanded = false" @keydown="key" @click="$event.target === dialog && close()">
    <ol>
      <li v-for="(marker, position) in markers" :key="marker.key">
        <button type="button" class="ui-menu-item" @click="choose(marker.index)">
          <span class="outline-number">{{ position + 1 }}</span>
          <span class="outline-prompt">{{ marker.prompt }}</span>
        </button>
      </li>
    </ol>
  </dialog>
</template>

<style scoped>
.session-outline { position: fixed; width: 320px; max-width: calc(100vw - 16px); max-height: min(60vh, 480px); margin: 0; padding: var(--space-1); overflow-y: auto; }
.session-outline::backdrop { background: transparent; }
.session-outline ol { margin: 0; padding: 0; list-style: none; }
.session-outline .ui-menu-item { align-items: flex-start; height: auto; padding-block: var(--space-2); }
.outline-number { flex: none; min-width: 1.5em; color: var(--muted-foreground); font-variant-numeric: tabular-nums; }
.outline-prompt { display: -webkit-box; min-width: 0; overflow: hidden; -webkit-line-clamp: 2; -webkit-box-orient: vertical; white-space: normal; text-align: left; }
</style>
