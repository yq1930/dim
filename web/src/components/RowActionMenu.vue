<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, type Component } from "vue"
import { MoreHorizontal } from "lucide-vue-next"
/** `icon` replaces the trigger's ⋯; `triggerClass` styles the trigger, since the menu has two roots. */
defineProps<{ label: string; icon?: Component; triggerClass?: string }>()
const trigger = ref<HTMLButtonElement>()
const dialog = ref<HTMLDialogElement>()
const expanded = ref(false)
const left = ref(0)
const top = ref(0)
function close(): void { dialog.value?.close(); expanded.value = false }
async function open(event?: MouseEvent): Promise<void> {
  const element = dialog.value
  if (!element) return
  const anchor = trigger.value!.getBoundingClientRect()
  left.value = Math.max(8, Math.min(event?.clientX ?? anchor.right - 208, window.innerWidth - 216))
  top.value = event?.clientY ?? anchor.bottom + 4
  element.showModal()
  expanded.value = true
  await nextTick()
  top.value = Math.max(8, Math.min(top.value, window.innerHeight - element.offsetHeight - 8))
  element.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus()
}
function key(event: KeyboardEvent): void {
  if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return
  event.preventDefault()
  const items = [...(dialog.value?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [])]
  const index = items.findIndex((item) => item === document.activeElement)
  const next = event.key === "Home" ? 0 : event.key === "End" ? items.length - 1 : (index + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length
  items[next]?.focus()
}
onBeforeUnmount(close)
defineExpose({ open })
</script>

<template>
  <button ref="trigger" type="button" class="ui-icon-button" :class="triggerClass" data-variant="ghost" data-size="sm" :aria-label="label" aria-haspopup="dialog" :aria-expanded="expanded" v-tooltip="label" @click.stop="open()"><component :is="icon ?? MoreHorizontal" aria-hidden="true" /></button>
  <dialog ref="dialog" class="row-action-menu ui-popover" :style="{ left: `${left}px`, top: `${top}px` }" :aria-label="label" @cancel="expanded = false" @close="expanded = false" @keydown="key" @click="($event.target === dialog || ($event.target as HTMLElement).closest('button')) && close()">
    <slot />
  </dialog>
</template>

<style scoped>
.row-action-menu { position: fixed; width: 208px; max-width: calc(100vw - 16px); margin: 0; padding: var(--space-1); }
.row-action-menu::backdrop { background: transparent; }
</style>
