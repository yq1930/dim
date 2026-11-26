<script setup lang="ts">
import { computed, nextTick, ref } from "vue"
import { useI18n } from "vue-i18n"
import type { QueuedInput } from "../../../rpc/protocol"
import { useChatStore } from "../../../stores/chat"
import { useToastStore } from "../../../stores/toast"

const props = defineProps<{ sessionId: string; items: readonly QueuedInput[]; running: boolean; editable: boolean }>()
const { t } = useI18n()
const chat = useChatStore()
const toast = useToastStore()
const busy = ref(false)
const draft = ref<{ item: QueuedInput; text: string }>()
const editor = ref<HTMLTextAreaElement>()
const notHeld = ref(false)
let editButton: HTMLElement | undefined
const hasAttachments = computed(() => draft.value?.item.user.content.some((block) => block.type !== "text") === true)
const unavailable = computed(() => notHeld.value || (draft.value !== undefined && !props.items.some((item) => item.id === draft.value?.item.id)))

async function edit(item: QueuedInput, event: MouseEvent): Promise<void> {
  editButton = event.currentTarget instanceof HTMLElement ? event.currentTarget : undefined
  const first = item.user.content[0]
  draft.value = { item, text: first?.type === "text" && typeof first.text === "string" ? first.text : "" }
  notHeld.value = false
  await nextTick()
  editor.value?.focus()
}

async function closeEditor(): Promise<void> {
  draft.value = undefined
  await nextTick()
  editButton?.focus()
  editButton = undefined
}

async function save(): Promise<void> {
  const editing = draft.value
  if (busy.value || unavailable.value || editing === undefined) return
  const content = editing.item.user.content.filter((block) => block.type !== "text")
  if (editing.text.trim() !== "") content.unshift({ type: "text", text: editing.text })
  if (content.length === 0) return
  busy.value = true
  try {
    const result = await chat.editQueuedInput(props.sessionId, editing.item.id, { content })
    if (result.status === "edited") void closeEditor()
    else notHeld.value = true
  } catch (cause) {
    toast.push({ tone: "danger", title: t("command.failed"), description: cause instanceof Error ? cause.message : String(cause) })
  } finally {
    busy.value = false
  }
}

function editKeys(event: KeyboardEvent): void {
  if (event.isComposing) return
  if (event.key === "Escape") {
    event.preventDefault()
    event.stopPropagation()
    if (!busy.value) void closeEditor()
  } else if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
    event.preventDefault()
    event.stopPropagation()
    void save()
  }
}

async function withdraw(item: QueuedInput, steer: boolean): Promise<void> {
  if (busy.value) return
  busy.value = true
  try {
    const result = await chat.withdrawInput(props.sessionId, item.id, steer ? item.user : undefined)
    if (result.status === "notHeld") toast.push({ tone: "info", title: t("queue.notHeld") })
  } catch (cause) {
    toast.push({ tone: "danger", title: t("command.failed"), description: cause instanceof Error ? cause.message : String(cause) })
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div v-if="items.length || draft" class="composer-panel">
    <section class="queued-inputs" :aria-label="t('queue.title')">
      <h2 class="queued-title">{{ t("queue.title") }}</h2>
      <ol class="queued-list">
        <li v-for="(item, index) in items" :key="item.id" class="queued-item">
          <span class="queued-index">{{ index + 1 }}</span>
          <span class="queued-text">{{ item.text }}</span>
          <span v-for="skill in item.skills" :key="skill" class="queued-skill">{{ t("message.skillLoaded", { name: skill }) }}</span>
          <button v-if="editable" type="button" class="ui-button" data-size="sm" data-variant="ghost" :disabled="busy || draft !== undefined" @click="edit(item, $event)">{{ t("queue.edit") }}</button>
          <button v-if="index === 0 && running" type="button" class="ui-button" data-size="sm" data-variant="secondary" :disabled="busy" @click="withdraw(item, true)">{{ t("queue.steer") }}</button>
          <button type="button" class="ui-button" data-size="sm" data-variant="ghost" :disabled="busy" @click="withdraw(item, false)">{{ t("queue.remove") }}</button>
        </li>
      </ol>
      <form v-if="draft" class="queue-editor" @submit.prevent="save" @keydown="editKeys">
        <textarea ref="editor" v-model="draft.text" class="ui-field" data-size="md" rows="3" :readonly="busy" :aria-label="t('queue.editMessage')" />
        <p v-if="unavailable && !busy" class="queue-edit-status" role="status">{{ t("queue.editUnavailable") }}</p>
        <div class="queue-edit-actions">
          <button type="button" class="ui-button" data-size="sm" data-variant="ghost" :disabled="busy" @click="closeEditor">{{ t("common.cancel") }}</button>
          <button type="submit" class="ui-button" data-size="sm" data-variant="primary" :disabled="busy || unavailable || (!draft.text.trim() && !hasAttachments)">{{ t("common.save") }}</button>
        </div>
      </form>
    </section>
  </div>
</template>

<style scoped>
.queued-inputs { display: grid; gap: var(--space-1); padding: var(--space-2) var(--space-3); border: 1px dashed var(--border); border-radius: var(--radius-lg); background: var(--muted); font-size: var(--text-sm); }
.queued-title { margin: 0; color: var(--muted-foreground); font-size: var(--text-xs); font-weight: var(--weight-medium); }
.queued-list { display: grid; gap: var(--space-1); margin: 0; padding: 0; list-style: none; }
.queued-item { display: flex; min-width: 0; align-items: center; gap: var(--space-2); }
.queued-index { color: var(--muted-foreground); font-variant-numeric: tabular-nums; }
.queued-text { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.queued-skill { color: var(--muted-foreground); font-size: var(--text-xs); white-space: nowrap; }
.queue-editor { display: grid; gap: var(--space-2); margin-top: var(--space-1); }
.queue-editor .ui-field { max-height: 180px; resize: vertical; }
.queue-edit-actions { display: flex; justify-content: flex-end; gap: var(--space-2); }
.queue-edit-status { margin: 0; color: var(--muted-foreground); }
</style>
