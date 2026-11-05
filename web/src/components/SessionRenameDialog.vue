<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, useId } from "vue"
import { useI18n } from "vue-i18n"
import { useConnectionStore } from "../stores/connection"
import { useSessionsStore } from "../stores/sessions"
import { readSessionMethods } from "../rpc/read"
import type { SessionSummary } from "../rpc/protocol"
const { t } = useI18n()
const connection = useConnectionStore()
const sessions = useSessionsStore()
const id = useId()
const dialog = ref<HTMLDialogElement>()
const field = ref<HTMLInputElement>()
const sessionId = ref("")
const title = ref("")
const initial = ref("")
const ready = ref(false)
const saving = ref(false)
const error = ref("")
let generation = 0
function close(): void { ++generation; dialog.value?.close() }
async function open(session: SessionSummary): Promise<void> {
  const request = ++generation
  sessionId.value = session.sessionId
  title.value = initial.value = session.title ?? session.preview ?? ""
  ready.value = false
  saving.value = false
  error.value = ""
  dialog.value?.showModal()
  await nextTick()
  field.value?.select()
  try {
    const methods = await connection.client.call("session.methods", { sessionId: session.sessionId }, readSessionMethods)
    if (request !== generation) return
    ready.value = methods.includes("session.rename")
    if (!ready.value) error.value = t("sidebar.renameUnavailable")
  } catch (cause) { if (request === generation) error.value = cause instanceof Error ? cause.message : String(cause) }
}
async function save(): Promise<void> {
  if (!ready.value || saving.value || !title.value.trim()) return
  const request = generation
  saving.value = true
  error.value = ""
  try { await sessions.rename(sessionId.value, title.value.trim()); if (request === generation) close() }
  catch (cause) { if (request === generation) error.value = cause instanceof Error ? cause.message : String(cause) }
  finally { if (request === generation) saving.value = false }
}
onBeforeUnmount(close)
defineExpose({ open })
</script>

<template>
  <dialog ref="dialog" class="ui-dialog" data-size="md" :aria-labelledby="id" @cancel.prevent="close">
    <form @submit.prevent="save">
      <header class="ui-dialog-header"><h2 :id="id" class="ui-dialog-title">{{ t("sidebar.rename") }}</h2></header>
      <div class="rename-form"><input ref="field" v-model="title" class="ui-field" :aria-label="t('sidebar.sessionName')" :disabled="saving" required /><p v-if="error" class="ui-banner" data-tone="danger" role="alert">{{ error }}</p></div>
      <footer class="ui-dialog-footer"><button type="button" class="ui-button" data-variant="secondary" @click="close">{{ t("common.cancel") }}</button><button class="ui-button" data-variant="primary" :disabled="!ready || saving || !title.trim() || title.trim() === initial || connection.phase !== 'connected'">{{ t("common.save") }}</button></footer>
    </form>
  </dialog>
</template>

<style scoped>
.rename-form { display: grid; gap: var(--space-2); padding: var(--space-4); }
</style>
