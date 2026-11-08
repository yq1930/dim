<script setup lang="ts">
import { nextTick, ref, useId } from "vue"
import { useI18n } from "vue-i18n"
import { canChooseWorkspace, chooseWorkspace } from "../../desktop"

const emit = defineEmits<{ select: [cwd: string] }>()
const { t } = useI18n()
const id = useId()
const dialog = ref<HTMLDialogElement>()
const field = ref<HTMLInputElement>()
const path = ref("")
const error = ref("")
const choosing = ref(false)
async function open(current = ""): Promise<void> {
  if (choosing.value) return
  path.value = current
  error.value = ""
  if (canChooseWorkspace) {
    choosing.value = true
    try {
      const chosen = await chooseWorkspace(current)
      if (chosen !== null) emit("select", chosen)
      return
    } catch (cause) { error.value = cause instanceof Error ? cause.message : String(cause) }
    finally { choosing.value = false }
  }
  dialog.value?.showModal()
  await nextTick()
  field.value?.focus()
}
function select(): void {
  const value = path.value.trim()
  if (!value.startsWith("/") && !/^[a-z]:[\\/]/i.test(value) && !value.startsWith("\\\\")) { error.value = t("workspace.absoluteRequired"); return }
  emit("select", value)
  dialog.value?.close()
}
defineExpose({ open })
</script>

<template>
  <dialog ref="dialog" class="ui-dialog" data-size="md" :aria-labelledby="id">
    <form @submit.prevent="select">
      <header class="ui-dialog-header"><h2 :id="id" class="ui-dialog-title">{{ t("workspace.open") }}</h2></header>
      <div class="workspace-form"><label>{{ t("newThread.pathLabel") }}<input ref="field" v-model="path" class="ui-field" required placeholder="/path/to/project" spellcheck="false" /></label><p v-if="error" class="ui-banner" data-tone="danger" role="alert">{{ error }}</p></div>
      <footer class="ui-dialog-footer"><button type="button" class="ui-button" data-variant="secondary" @click="dialog?.close()">{{ t("common.cancel") }}</button><button class="ui-button" data-variant="primary" :disabled="!path.trim()">{{ t("workspace.open") }}</button></footer>
    </form>
  </dialog>
</template>

<style scoped>
.workspace-form { display: grid; gap: var(--space-3); padding: var(--space-4); }
.workspace-form label { display: grid; gap: var(--space-2); }
</style>
