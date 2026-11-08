<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, useId, watch } from "vue"
import { useI18n } from "vue-i18n"
import { FileText, Folder, Search } from "lucide-vue-next"
import type { FileCandidate } from "../../rpc/protocol"
const props = defineProps<{ search: (query: string) => Promise<FileCandidate[]>; title?: string }>()
const emit = defineEmits<{ select: [file: FileCandidate] }>()
const { t } = useI18n()
const id = useId()
const dialog = ref<HTMLDialogElement>()
const field = ref<HTMLInputElement>()
const query = ref("")
const candidates = ref<FileCandidate[]>([])
const loading = ref(false)
const error = ref("")
let sequence = 0
let timer: ReturnType<typeof setTimeout> | undefined
async function load(): Promise<void> {
  const request = ++sequence
  loading.value = true
  error.value = ""
  try { const found = await props.search(query.value); if (request === sequence) candidates.value = found }
  catch (cause) { if (request === sequence) error.value = cause instanceof Error ? cause.message : String(cause) }
  finally { if (request === sequence) loading.value = false }
}
watch(query, () => {
  ++sequence
  clearTimeout(timer)
  loading.value = true
  candidates.value = []
  timer = setTimeout(() => void load(), 150)
})
async function open(): Promise<void> {
  query.value = ""
  candidates.value = []
  dialog.value?.showModal()
  await nextTick()
  clearTimeout(timer)
  field.value?.focus()
  void load()
}
function close(): void { ++sequence; clearTimeout(timer); loading.value = false; dialog.value?.close() }
function select(candidate: FileCandidate): void {
  if (candidate.dir) { query.value = `${candidate.relativePath}/`; return }
  emit("select", candidate)
  close()
}
onBeforeUnmount(close)
defineExpose({ open, close })
</script>

<template>
  <dialog ref="dialog" class="ui-dialog file-dialog" data-size="lg" :aria-labelledby="id" @cancel.prevent="close" @close="close">
    <header class="ui-dialog-header"><h2 :id="id" class="ui-dialog-title">{{ title ?? t("composer.workspaceFiles") }}</h2></header>
    <div class="file-search ui-field-wrap"><Search aria-hidden="true" /><input ref="field" v-model="query" class="ui-field" :placeholder="t('composer.searchFiles')" :aria-label="t('composer.searchFiles')" /></div>
    <div class="file-results">
      <p v-if="loading" role="status">{{ t("common.loading") }}</p>
      <p v-else-if="error" class="ui-banner" data-tone="danger" role="alert">{{ error }}</p>
      <p v-else-if="!candidates.length" role="status">{{ t("composer.noFiles") }}</p>
      <button v-for="candidate in candidates" :key="candidate.absolutePath" type="button" class="ui-menu-item" @click="select(candidate)"><Folder v-if="candidate.dir" aria-hidden="true" /><FileText v-else aria-hidden="true" /><span>{{ candidate.relativePath }}</span></button>
    </div>
    <footer class="ui-dialog-footer"><button type="button" class="ui-button" data-variant="secondary" @click="close">{{ t("common.close") }}</button></footer>
  </dialog>
</template>

<style scoped>
.file-search { margin: var(--space-3) var(--space-4); }
.file-results { min-height: 120px; max-height: 50dvh; overflow: auto; padding: 0 var(--space-4); }
.file-results p { padding: var(--space-2); color: var(--muted-foreground); }
.file-results .ui-menu-item span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: var(--font-mono); }
</style>
