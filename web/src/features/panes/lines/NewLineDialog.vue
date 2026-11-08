<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue"
import { useI18n } from "vue-i18n"
import { GitBranch } from "lucide-vue-next"
import { MAIN_BRANCH } from "../../../rpc/protocol"
import { useChatStore } from "../../../stores/chat"
import { usePanesStore } from "../../../stores/panes"
import { useUiStore } from "../../../stores/ui"
import { lineName, shortId } from "./line-state"

const { t } = useI18n()
const chat = useChatStore()
const ui = useUiStore()
const dialog = ref<HTMLDialogElement>()
const prompt = ref("")
const error = ref("")
const creating = ref(false)
const request = computed(() => ui.newLine)

const parentName = computed(() => {
  const current = request.value
  if (current === undefined) return ""
  if (current.parentBranchId === MAIN_BRANCH) return t("lines.main")
  const parent = chat.views[current.sessionId]?.lines.find((line) => line.id === current.parentBranchId)
  return parent === undefined ? shortId(current.parentBranchId) : lineName(t, parent, current.parentBranchId)
})

const cutLabel = computed(() => {
  const current = request.value
  if (current === undefined) return ""
  const before = current.before ?? current.text.split("\n")[0]?.trim() ?? ""
  return before === "" ? t("lines.atEnd") : t("lines.beforeMessage", { message: before })
})

function reset(): void {
  prompt.value = request.value?.text ?? ""
  error.value = ""
  creating.value = false
}

watch(request, reset, { immediate: true })
onMounted(() => dialog.value?.showModal())

function close(): void {
  dialog.value?.close()
}

async function create(): Promise<void> {
  const current = request.value
  if (current === undefined || creating.value) return
  const text = prompt.value.trim()
  if (text === "") {
    error.value = t("lines.promptRequired")
    return
  }
  creating.value = true
  error.value = ""
  try {
    const branchId = await chat.createLine(current.sessionId, current.parentBranchId, current.at, text)
    usePanesStore().openLine(current.sessionId, branchId)
    close()
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause)
  } finally {
    creating.value = false
  }
}
</script>

<template>
  <dialog ref="dialog" class="ui-dialog" data-size="md" @cancel.prevent="close" @close="ui.closeNewLine()">
    <form v-if="request" @submit.prevent="create">
      <header class="ui-dialog-header">
        <span class="ui-dialog-icon" data-tone="primary"><GitBranch :size="16" aria-hidden="true" /></span>
        <div>
          <h2 class="ui-dialog-title">{{ t("lines.newLineTitle") }}</h2>
          <p class="ui-dialog-description">
            {{ t("lines.newLineFrom", { name: parentName }) }}<br>
            <span class="new-line-cut">{{ cutLabel }}</span>
          </p>
        </div>
        <button type="button" class="ui-icon-button ui-dialog-close" data-variant="close" data-size="sm" :aria-label="t('common.close')" @click="close">
          <span aria-hidden="true">×</span>
        </button>
      </header>

      <label class="new-line-label" for="new-line-prompt">{{ t("lines.promptLabel") }}</label>
      <textarea
        id="new-line-prompt"
        v-model="prompt"
        class="ui-field new-line-input"
        rows="4"
        :placeholder="t('lines.promptPlaceholder')"
        autofocus
      />

      <p v-if="error" class="ui-banner" data-tone="danger" role="alert">{{ error }}</p>

      <footer class="ui-dialog-footer">
        <button type="button" class="ui-button" data-variant="ghost" data-size="md" :disabled="creating" @click="close">{{ t("common.cancel") }}</button>
        <button type="submit" class="ui-button" data-variant="primary" data-size="md" :disabled="creating || prompt.trim() === ''">
          {{ creating ? t("common.loading") : t("lines.create") }}
        </button>
      </footer>
    </form>
  </dialog>
</template>

<style scoped>
.new-line-label { display: block; margin-top: var(--space-4); color: var(--foreground); font-size: var(--text-sm); font-weight: var(--weight-medium); }
.new-line-input { width: 100%; min-height: 96px; margin-top: var(--space-2); resize: vertical; }
.new-line-cut { color: var(--muted-foreground); font-family: var(--font-mono); font-size: var(--text-xs); }
</style>
