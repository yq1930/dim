<script setup lang="ts">
import { computed, onMounted, ref } from "vue"
import { useI18n } from "vue-i18n"
import { GitBranch } from "lucide-vue-next"
import { useChatStore } from "../../../stores/chat"
import { useUiStore } from "../../../stores/ui"

const { t } = useI18n()
const chat = useChatStore()
const ui = useUiStore()
const dialog = ref<HTMLDialogElement>()
const error = ref("")
const ending = ref(false)
const request = computed(() => ui.endingLine)

onMounted(() => dialog.value?.showModal())

function close(): void {
  dialog.value?.close()
}

async function end(): Promise<void> {
  const current = request.value
  if (current === undefined || ending.value) return
  ending.value = true
  error.value = ""
  try {
    await chat.endLine(current.sessionId, current.branchId)
    close()
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause)
  } finally {
    ending.value = false
  }
}
</script>

<template>
  <dialog ref="dialog" class="ui-dialog" @cancel.prevent="close" @close="ui.closeEndLine()">
    <div v-if="request">
      <header class="ui-dialog-header">
        <span class="ui-dialog-icon" data-tone="danger"><GitBranch :size="16" aria-hidden="true" /></span>
        <div>
          <h2 class="ui-dialog-title">{{ t("lines.endTitle", { name: request.purpose }) }}</h2>
          <p class="ui-dialog-description">{{ t("lines.endDescription") }}</p>
        </div>
        <button type="button" class="ui-icon-button ui-dialog-close" data-variant="close" data-size="sm" :aria-label="t('common.close')" @click="close">
          <span aria-hidden="true">×</span>
        </button>
      </header>
      <p v-if="error" class="ui-banner" data-tone="danger" role="alert">{{ error }}</p>
      <footer class="ui-dialog-footer">
        <button type="button" class="ui-button" data-variant="ghost" data-size="md" :disabled="ending" @click="close">{{ t("common.cancel") }}</button>
        <button type="button" class="ui-button" data-variant="danger" data-size="md" :disabled="ending" @click="end">{{ t("lines.end") }}</button>
      </footer>
    </div>
  </dialog>
</template>
