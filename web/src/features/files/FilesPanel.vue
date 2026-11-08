<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue"
import { useI18n } from "vue-i18n"
import { FileText, Search, X } from "lucide-vue-next"
import type { FileCandidate } from "../../rpc/protocol"
import { useChatStore } from "../../stores/chat"
import { useFilesStore, type OpenFile } from "../../stores/files"
import { useToastStore } from "../../stores/toast"
import WorkspaceFileDialog from "../chat/WorkspaceFileDialog.vue"
import FileViewer from "./FileViewer.vue"

/** The detail panel's file view of a Session: a tab per open file, the active one shown and read again whenever it is shown. */
const props = defineProps<{ sessionId: string }>()
const { t } = useI18n()
const chat = useChatStore()
const files = useFilesStore()
const toast = useToastStore()
const finder = ref<InstanceType<typeof WorkspaceFileDialog>>()
const strip = ref<HTMLElement>()
const list = computed(() => files.bySession[props.sessionId] ?? [])
const current = computed(() => list.value.find((file) => file.path === files.active[props.sessionId]))
const session = computed(() => chat.views[props.sessionId])
const searchable = computed(() => session.value?.methods.includes("session.files.search") === true)
const running = computed(() => session.value?.sessionRunning === true || session.value?.running === true)

function name(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1)
}

/** The folder a tab names beside its file when another open file has the same name. */
function folder(file: OpenFile): string | undefined {
  const own = name(file.path)
  if (list.value.filter((other) => name(other.path) === own).length < 2) return undefined
  const shown = file.view?.relativePath ?? file.path
  const directory = shown.slice(0, Math.max(0, shown.lastIndexOf("/")))
  return name(directory) || undefined
}

function search(query: string): Promise<FileCandidate[]> {
  return chat.searchFiles(props.sessionId, query)
}

async function pick(candidate: FileCandidate): Promise<void> {
  try {
    await files.open(props.sessionId, candidate.absolutePath, undefined, true)
  } catch (cause) {
    toast.push({ tone: "danger", title: t("chat.openFailed"), description: cause instanceof Error ? cause.message : String(cause) })
  }
}

watch(() => current.value?.path, async (path) => {
  if (path === undefined) return
  void files.reload(props.sessionId, path)
  await nextTick()
  strip.value?.querySelector(".is-active")?.scrollIntoView({ block: "nearest", inline: "nearest" })
}, { immediate: true })
watch(running, (now, before) => { if (before && !now && current.value !== undefined) void files.reload(props.sessionId, current.value.path) })
</script>

<template>
  <div class="files-panel">
    <header class="panel-subtabs">
      <div ref="strip" class="panel-tablist" role="tablist" :aria-label="t('files.tabs')">
        <div v-for="file in list" :key="file.path" class="panel-tab-wrap" :class="{ 'is-active': file.path === current?.path }">
          <button
            type="button"
            role="tab"
            class="panel-tab"
            :aria-selected="file.path === current?.path"
            :data-preview="file.preview"
            :title="file.view?.relativePath ?? file.path"
            @click="files.activate(sessionId, file.path)"
            @dblclick="files.keepOpen(sessionId, file.path)"
            @click.middle="files.close(sessionId, file.path)"
          >
            <FileText :size="14" aria-hidden="true" />
            <span class="panel-tab-title">{{ name(file.path) }}</span>
            <span v-if="folder(file)" class="panel-tab-detail">{{ folder(file) }}</span>
          </button>
          <button type="button" class="panel-tab-close" :aria-label="t('files.close')" @click="files.close(sessionId, file.path)"><X :size="12" aria-hidden="true" /></button>
        </div>
      </div>
      <button v-if="searchable" type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('files.find')" v-tooltip="t('files.find')" @click="finder?.open()">
        <Search aria-hidden="true" />
      </button>
    </header>
    <FileViewer v-if="current" :key="current.path" :session-id="sessionId" :file="current" />
    <p v-else class="panel-empty">{{ t("files.empty") }}</p>
    <WorkspaceFileDialog v-if="searchable" ref="finder" :search="search" :title="t('files.find')" @select="pick" />
  </div>
</template>

<style scoped>
.files-panel { display: flex; min-width: 0; min-height: 0; flex-direction: column; background: var(--background); }
.files-panel > .file-viewer { flex: 1; }
</style>
