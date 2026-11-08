<script setup lang="ts">
import { computed, watch } from "vue"
import { useI18n } from "vue-i18n"
import { ChevronRight, ChevronsDownUp, FileText, Folder, FolderOpen, LoaderCircle, RefreshCw } from "lucide-vue-next"
import { useFilesStore } from "../../../stores/files"
import { useToastStore } from "../../../stores/toast"
import { workspaceName } from "../../../stores/sessions"

/**
 * The Session's workspace as a tree: a folder opens in place, a file opens in the
 * file view (a double click keeps its tab). Folders are read when first opened.
 */
const props = defineProps<{ sessionId: string; root: string; activePath?: string }>()
const emit = defineEmits<{ opened: [] }>()
const { t } = useI18n()
const files = useFilesStore()
const toast = useToastStore()

type Row = { key: string; path: string; name: string; depth: number } & ({ kind: "file" } | { kind: "folder"; open: boolean } | { kind: "loading" } | { kind: "error"; message: string } | { kind: "more" })

const rows = computed<Row[]>(() => {
  const folders = files.folders[props.sessionId] ?? {}
  const open = new Set(files.expanded[props.sessionId] ?? [])
  const found: Row[] = []
  const walk = (folder: string, depth: number): void => {
    const read = folders[folder]
    if (read?.error !== undefined) {
      found.push({ key: `${folder}\0error`, path: folder, name: "", depth, kind: "error", message: read.error })
      return
    }
    if (read?.listing === undefined) {
      found.push({ key: `${folder}\0loading`, path: folder, name: "", depth, kind: "loading" })
      return
    }
    for (const entry of read.listing.entries) {
      const path = folder === "" ? entry.name : `${folder}/${entry.name}`
      if (!entry.dir) {
        found.push({ key: path, path, name: entry.name, depth, kind: "file" })
        continue
      }
      const expanded = open.has(path)
      found.push({ key: path, path, name: entry.name, depth, kind: "folder", open: expanded })
      if (expanded) walk(path, depth + 1)
    }
    if (read.listing.truncated) found.push({ key: `${folder}\0more`, path: folder, name: "", depth, kind: "more" })
  }
  walk("", 0)
  return found
})
const loading = computed(() => files.folders[props.sessionId]?.[""] === undefined)

watch(() => props.sessionId, (sessionId) => {
  if (files.folders[sessionId]?.[""] === undefined) void files.readFolder(sessionId, "")
}, { immediate: true })

async function openFile(path: string, keep: boolean): Promise<void> {
  try {
    if (await files.open(props.sessionId, path, undefined, keep)) emit("opened")
  } catch (cause) {
    toast.push({ tone: "danger", title: t("chat.openFailed"), description: cause instanceof Error ? cause.message : String(cause) })
  }
}
</script>

<template>
  <nav class="file-tree" :aria-label="t('files.tree')">
    <div class="file-tree-head">
      <span class="file-tree-root" :title="root">{{ workspaceName(root) }}</span>
      <button type="button" class="ui-icon-button" data-variant="ghost" data-size="xs" :aria-label="t('files.collapseTree')" v-tooltip="t('files.collapseTree')" @click="files.collapseTree(sessionId)"><ChevronsDownUp aria-hidden="true" /></button>
      <button type="button" class="ui-icon-button" data-variant="ghost" data-size="xs" :aria-label="t('files.refreshTree')" v-tooltip="t('files.refreshTree')" :disabled="loading" @click="files.refreshTree(sessionId)"><RefreshCw aria-hidden="true" /></button>
    </div>
    <ul class="file-tree-list" role="tree">
      <li v-for="row in rows" :key="row.key" role="none">
        <button
          v-if="row.kind === 'folder'"
          type="button"
          role="treeitem"
          class="file-tree-row"
          :aria-expanded="row.open"
          :style="{ '--depth': row.depth }"
          :title="row.path"
          @click="files.toggleFolder(sessionId, row.path)"
        >
          <ChevronRight class="file-tree-chevron" :class="{ 'is-open': row.open }" aria-hidden="true" />
          <FolderOpen v-if="row.open" aria-hidden="true" /><Folder v-else aria-hidden="true" />
          <span>{{ row.name }}</span>
        </button>
        <button
          v-else-if="row.kind === 'file'"
          type="button"
          role="treeitem"
          class="file-tree-row"
          :aria-current="row.path === activePath ? 'true' : undefined"
          :style="{ '--depth': row.depth }"
          :title="row.path"
          @click="openFile(row.path, false)"
          @dblclick="openFile(row.path, true)"
        >
          <span class="file-tree-chevron" aria-hidden="true" />
          <FileText aria-hidden="true" />
          <span>{{ row.name }}</span>
        </button>
        <p v-else-if="row.kind === 'loading'" class="file-tree-note" :style="{ '--depth': row.depth }"><LoaderCircle class="is-spinning" aria-hidden="true" />{{ t("common.loading") }}</p>
        <p v-else-if="row.kind === 'error'" class="file-tree-note" data-tone="danger" :style="{ '--depth': row.depth }">{{ row.message }}</p>
        <p v-else class="file-tree-note" :style="{ '--depth': row.depth }">{{ t("files.treeTruncated") }}</p>
      </li>
    </ul>
  </nav>
</template>

<style scoped>
.file-tree { display: flex; min-width: 0; min-height: 0; flex-direction: column; background: var(--background); }
.file-tree-head { display: flex; height: 36px; flex: none; align-items: center; gap: var(--space-1); padding: 0 var(--space-2) 0 var(--space-3); border-bottom: 1px solid var(--border); }
.file-tree-root { min-width: 0; flex: 1; overflow: hidden; color: var(--muted-foreground); font-size: var(--text-xs); font-weight: var(--weight-medium); text-overflow: ellipsis; white-space: nowrap; }
.file-tree-list { min-height: 0; flex: 1; margin: 0; padding: var(--space-1) 0 var(--space-2); overflow: auto; list-style: none; }
.file-tree-row { display: flex; width: 100%; min-width: 0; height: 26px; align-items: center; gap: 6px; padding: 0 var(--space-2) 0 calc(var(--space-2) + var(--depth) * 14px); border: 0; background: none; color: var(--foreground); font: inherit; font-size: var(--text-sm); text-align: left; cursor: pointer; }
.file-tree-row:hover { background: var(--hover); }
.file-tree-row[aria-current="true"] { background: var(--muted); font-weight: var(--weight-medium); }
.file-tree-row > svg { width: 14px; height: 14px; flex: none; color: var(--muted-foreground); }
.file-tree-row > span:last-child { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.file-tree-chevron { width: 12px !important; height: 12px !important; flex: none; transition: transform var(--duration-press) var(--ease-out); }
.file-tree-chevron.is-open { transform: rotate(90deg); }
.file-tree-note { display: flex; align-items: center; gap: 6px; margin: 0; padding: 4px var(--space-2) 4px calc(var(--space-2) + 20px + var(--depth) * 14px); color: var(--muted-foreground); font-size: var(--text-xs); }
.file-tree-note[data-tone="danger"] { color: var(--danger); }
.file-tree-note > svg { width: 12px; height: 12px; }
@media (prefers-reduced-motion: reduce) { .file-tree-chevron { transition: none; } }
</style>
