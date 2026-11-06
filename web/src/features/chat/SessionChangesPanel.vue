<script setup lang="ts">
import { computed, ref, watch } from "vue"
import { useI18n } from "vue-i18n"
import { ChevronRight, FileDiff, Maximize2, Minimize2, X } from "lucide-vue-next"
import { useChatStore } from "../../stores/chat"
import { useSessionsStore } from "../../stores/sessions"
import { useUiStore } from "../../stores/ui"
import DiffLines from "./DiffLines.vue"
import { sessionChanges, workspacePath } from "./session-changes"

/**
 * The detail panel's review of what the Session's main line changed: every
 * file its `write` and `edit` calls touched, with each call's diff in order.
 */
const props = defineProps<{ sessionId: string }>()
const { t } = useI18n()
const chat = useChatStore()
const sessions = useSessionsStore()
const ui = useUiStore()

const view = computed(() => chat.views[props.sessionId])
const cwd = computed(() => sessions.byId.get(props.sessionId)?.cwd)
const files = computed(() => sessionChanges(view.value?.rows ?? [], cwd.value))
const totals = computed(() => files.value.reduce((sum, file) => ({ added: sum.added + file.added, removed: sum.removed + file.removed }), { added: 0, removed: 0 }))
/** Files the reader folded; every file starts open. */
const folded = ref<Record<string, boolean>>({})
watch(() => props.sessionId, () => { folded.value = {} })
</script>

<template>
  <div class="changes-panel" role="region" :aria-label="t('sessionChanges.title')">
    <header class="changes-header">
      <FileDiff :size="14" aria-hidden="true" />
      <strong>{{ t("sessionChanges.title") }}</strong>
      <span v-if="files.length" class="changes-summary">
        {{ t("sessionChanges.files", { count: files.length }, files.length) }}
        <span class="changes-stat" data-tone="success">+{{ totals.added }}</span>
        <span class="changes-stat" data-tone="danger">−{{ totals.removed }}</span>
      </span>
      <div class="changes-actions">
        <button type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="ui.detailExpanded ? t('lines.collapse') : t('lines.expand')" v-tooltip="ui.detailExpanded ? t('lines.collapse') : t('lines.expand')" @click="ui.detailExpanded = !ui.detailExpanded">
          <Minimize2 v-if="ui.detailExpanded" aria-hidden="true" />
          <Maximize2 v-else aria-hidden="true" />
        </button>
        <button type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('sessionChanges.close')" v-tooltip="t('sessionChanges.close')" @click="ui.closeDetail">
          <X aria-hidden="true" />
        </button>
      </div>
    </header>

    <div class="changes-body app-scroll-viewport">
      <div v-if="!view || view.status === 'loading'" class="changes-empty" role="status">{{ t("common.loading") }}</div>
      <p v-else-if="files.length === 0" class="changes-empty">{{ t("sessionChanges.empty") }}</p>
      <section v-for="file in files" :key="file.path" class="changes-file">
        <button type="button" class="changes-file-toggle" :aria-expanded="!folded[file.path]" :title="file.path" @click="folded[file.path] = !folded[file.path]">
          <ChevronRight :size="14" :class="{ 'is-expanded': !folded[file.path] }" aria-hidden="true" />
          <span class="changes-file-path">{{ workspacePath(file.path, cwd) }}</span>
          <span class="changes-stat" data-tone="success">+{{ file.added }}</span>
          <span v-if="file.removed" class="changes-stat" data-tone="danger">−{{ file.removed }}</span>
        </button>
        <div v-if="!folded[file.path]" class="changes-file-body">
          <template v-for="(edit, index) in file.edits" :key="edit.key">
            <hr v-if="index > 0" class="changes-separator">
            <DiffLines :lines="edit.lines" />
            <p v-if="edit.hidden" class="changes-hidden">{{ t("sessionChanges.hiddenLines", { count: edit.hidden }, edit.hidden) }}</p>
          </template>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.changes-panel { display: flex; min-width: 0; min-height: 0; flex-direction: column; background: var(--background); }
.changes-header { display: flex; min-width: 0; height: 48px; flex: none; align-items: center; gap: var(--space-2); padding: 0 var(--space-2) 0 var(--space-3); border-bottom: 1px solid var(--border); font-size: var(--text-sm); }
.changes-header > svg { flex: none; color: var(--muted-foreground); }
.changes-summary { display: flex; min-width: 0; align-items: center; gap: var(--space-1); overflow: hidden; color: var(--muted-foreground); white-space: nowrap; }
.changes-actions { display: flex; flex: none; align-items: center; gap: 1px; margin-left: auto; }
.changes-body { display: flex; min-height: 0; flex: 1; flex-direction: column; gap: var(--space-2); padding: var(--space-3); overflow-y: auto; }
.changes-empty { margin: auto; color: var(--muted-foreground); font-size: var(--text-sm); }
.changes-file { flex: none; overflow: hidden; border: 1px solid var(--border); border-radius: var(--radius-lg); background: var(--card); }
.changes-file-toggle { display: flex; width: 100%; min-width: 0; height: var(--control-md); align-items: center; gap: var(--space-2); padding: 0 var(--space-3) 0 var(--space-2); border: 0; background: var(--muted); color: var(--foreground); font: inherit; font-size: var(--text-sm); text-align: left; cursor: pointer; }
.changes-file-toggle > svg { flex: none; color: var(--muted-foreground); transition: transform var(--duration-press) var(--ease-out); }
.changes-file-toggle > svg.is-expanded { transform: rotate(90deg); }
.changes-file-path { min-width: 0; flex: 1; overflow: hidden; font-family: var(--font-mono); font-size: var(--text-xs); text-overflow: ellipsis; white-space: nowrap; }
.changes-stat { flex: none; font-family: var(--font-mono); font-size: var(--text-xs); font-variant-numeric: tabular-nums; }
.changes-stat[data-tone="success"] { color: var(--success); }
.changes-stat[data-tone="danger"] { color: var(--danger); }
.changes-file-body { border-top: 1px solid var(--border); overflow-x: auto; }
.changes-separator { margin: 0; border: 0; border-top: 1px dashed var(--border); }
.changes-hidden { margin: 0; padding: 0 var(--space-3) var(--space-2); color: var(--muted-foreground); font-size: var(--text-xs); }
</style>
