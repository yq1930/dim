<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue"
import { useI18n } from "vue-i18n"
import { FileText, PanelLeft, Search, X } from "lucide-vue-next"
import ConfirmDialog from "../../../components/ConfirmDialog.vue"
import type { TextMatch } from "../../../rpc/files"
import type { FileCandidate } from "../../../rpc/protocol"
import { useChatStore } from "../../../stores/chat"
import { useFilesStore, type OpenFile } from "../../../stores/files"
import { useSessionsStore } from "../../../stores/sessions"
import { useToastStore } from "../../../stores/toast"
import WorkspaceFileDialog from "../../../components/WorkspaceFileDialog.vue"
import FileTree from "./FileTree.vue"
import FileViewer from "./FileViewer.vue"

/**
 * The detail panel's file view of a Session: the workspace's file tree, a tab per
 * open file, the active one shown and read again whenever it is shown. A wide
 * panel shows the tree beside the file; a narrow one over it, until a file opens.
 */
const props = defineProps<{ sessionId: string }>()
const { t } = useI18n()
const chat = useChatStore()
const files = useFilesStore()
const sessions = useSessionsStore()
const toast = useToastStore()
const finder = ref<InstanceType<typeof WorkspaceFileDialog>>()
const confirm = ref<InstanceType<typeof ConfirmDialog>>()
const panel = ref<HTMLElement>()
const strip = ref<HTMLElement>()
const list = computed(() => files.bySession[props.sessionId] ?? [])
const current = computed(() => list.value.find((file) => file.path === files.active[props.sessionId]))
const session = computed(() => chat.views[props.sessionId])
const searchable = computed(() => session.value?.methods.includes("session.files.search") === true)
const listable = computed(() => files.supports(props.sessionId, "session.files.list"))
const running = computed(() => session.value?.sessionRunning === true || session.value?.running === true)
const root = computed(() => sessions.byId.get(props.sessionId)?.cwd ?? "")
/** Below this width the tree shows over the file instead of beside it. */
const WIDE_PX = 600
const wide = ref(true)
/** The tree shown over the file on a narrow panel. */
const drawer = ref(false)
const showTree = computed(() => listable.value && (current.value === undefined || (wide.value ? files.tree : drawer.value)))
let observer: ResizeObserver | undefined

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

function toggleTree(): void {
  if (wide.value) files.tree = !files.tree
  else drawer.value = !drawer.value
}

function search(query: string): Promise<FileCandidate[]> {
  return chat.searchFiles(props.sessionId, query)
}

async function pick(candidate: FileCandidate): Promise<void> {
  await show(candidate.absolutePath)
}

async function show(path: string, lines?: { line: number }): Promise<void> {
  try {
    if (await files.open(props.sessionId, path, lines, true)) drawer.value = false
  } catch (cause) {
    toast.push({ tone: "danger", title: t("chat.openFailed"), description: cause instanceof Error ? cause.message : String(cause) })
  }
}

function match(found: TextMatch): Promise<void> {
  return show(found.path, { line: found.line })
}

/** Closes a tab, asking first when it has unsaved changes. */
async function close(file: OpenFile): Promise<void> {
  if (file.draft !== undefined && !await confirm.value?.open(t("files.discardConfirm", { name: name(file.path) }), { confirmLabel: t("files.discard") })) return
  files.close(props.sessionId, file.path)
}

watch(() => props.sessionId, (sessionId) => { void files.restore(sessionId) }, { immediate: true })
watch(() => current.value?.path, async (path) => {
  if (path === undefined) return
  void files.reload(props.sessionId, path)
  await nextTick()
  strip.value?.querySelector(".is-active")?.scrollIntoView({ block: "nearest", inline: "nearest" })
}, { immediate: true })
// A run may add, change or remove files; read the tree and the shown file again once it ends.
watch(running, (now, before) => {
  if (!before || now) return
  if (current.value !== undefined) void files.reload(props.sessionId, current.value.path)
  if (listable.value && files.folders[props.sessionId] !== undefined) void files.refreshTree(props.sessionId)
})
onMounted(() => {
  observer = new ResizeObserver(([entry]) => { if (entry !== undefined) wide.value = entry.contentRect.width >= WIDE_PX })
  if (panel.value !== undefined) observer.observe(panel.value)
})
onBeforeUnmount(() => observer?.disconnect())
</script>

<template>
  <div ref="panel" class="files-panel">
    <ConfirmDialog ref="confirm" />
    <header class="panel-subtabs">
      <button v-if="listable && current" type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-pressed="showTree" :aria-label="showTree ? t('files.hideTree') : t('files.showTree')" v-tooltip="showTree ? t('files.hideTree') : t('files.showTree')" @click="toggleTree">
        <PanelLeft aria-hidden="true" />
      </button>
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
            @click.middle="close(file)"
          >
            <FileText :size="14" aria-hidden="true" />
            <span class="panel-tab-title">{{ name(file.path) }}</span>
            <span v-if="folder(file)" class="panel-tab-detail">{{ folder(file) }}</span>
            <span v-if="file.draft !== undefined" class="panel-tab-dirty" :aria-label="t('files.unsaved')" />
          </button>
          <button type="button" class="panel-tab-close" :aria-label="t('files.close')" @click="close(file)"><X :size="12" aria-hidden="true" /></button>
        </div>
      </div>
      <button v-if="searchable" type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('files.find')" v-tooltip="t('files.find')" @click="finder?.open()">
        <Search aria-hidden="true" />
      </button>
    </header>
    <div class="files-body" :data-tree="showTree" :data-layout="wide || !current ? 'side' : 'over'">
      <FileTree v-if="showTree" class="files-tree" :session-id="sessionId" :root="root" :active-path="current?.view?.relativePath" @opened="drawer = false" />
      <FileViewer v-if="current" :key="current.path" :session-id="sessionId" :file="current" />
      <p v-else-if="!listable" class="panel-empty">{{ t("files.empty") }}</p>
    </div>
    <WorkspaceFileDialog v-if="searchable" ref="finder" :search="search" :grep="files.supports(sessionId, 'session.files.grep') ? (query) => files.grep(sessionId, query) : undefined" :title="t('files.find')" @select="pick" @match="match" />
  </div>
</template>

<style scoped>
.files-panel { display: flex; min-width: 0; min-height: 0; flex-direction: column; background: var(--background); }
.files-body { position: relative; display: flex; min-width: 0; min-height: 0; flex: 1; }
.files-body > .file-viewer { min-width: 0; flex: 1; }
.files-body > .panel-empty { align-self: center; }
.files-tree { width: 100%; flex: 1; }
.files-body[data-tree="true"][data-layout="side"]:has(.file-viewer) > .files-tree { width: clamp(180px, 30%, 280px); flex: none; border-right: 1px solid var(--border); }
.files-body[data-layout="over"] > .files-tree { position: absolute; z-index: 3; inset: 0 auto 0 0; width: min(300px, 85%); border-right: 1px solid var(--border); box-shadow: var(--shadow-pop); }
.panel-tab-dirty { width: 7px; height: 7px; flex: none; margin-left: 2px; border-radius: 50%; background: var(--foreground); opacity: .7; }
</style>
