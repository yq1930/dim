<script setup lang="ts">
import { computed, watch } from "vue"
import { useI18n } from "vue-i18n"
import { GitFork } from "lucide-vue-next"
import { useGitStore } from "../../stores/git"
import { workspaceName } from "../../stores/sessions"
import { useUiStore } from "../../stores/ui"

/**
 * Where a Session runs, before its title: the workspace (the main repository
 * of a worktree) and the Git branch, which opens the Git tab.
 */
const props = defineProps<{ cwd: string; interactive: boolean }>()
const { t } = useI18n()
const git = useGitStore()
const ui = useUiStore()
watch(() => props.cwd, (cwd) => { void git.inspect([cwd]) }, { immediate: true })
const place = computed(() => git.places[props.cwd])
const workspace = computed(() => git.linkedMain(props.cwd) ?? props.cwd)
const branch = computed(() => place.value?.root === undefined ? undefined : place.value.branch ?? t("git.detached"))
</script>

<template>
  <span class="workspace-crumb" :title="cwd">{{ workspaceName(workspace) }}</span>
  <button v-if="branch !== undefined && interactive" type="button" class="branch-chip" :aria-pressed="ui.detailOpen && ui.detailView === 'git'" :title="branch" @click="ui.toggleDetail('git')">
    <GitFork :size="12" aria-hidden="true" /><span>{{ branch }}</span>
  </button>
  <span v-else-if="branch !== undefined" class="branch-chip" :title="branch"><GitFork :size="12" aria-hidden="true" /><span>{{ branch }}</span></span>
  <span class="workspace-crumb-separator" aria-hidden="true">/</span>
</template>

<style scoped>
.branch-chip { display: inline-flex; min-width: 0; max-width: 220px; flex: 0 1 auto; align-items: center; gap: 4px; height: 22px; margin-left: var(--space-1); padding: 0 var(--space-2); border: 1px solid var(--border); border-radius: var(--radius-pill); background: transparent; color: var(--muted-foreground); font: inherit; font-family: var(--font-mono); font-size: var(--text-2xs); white-space: nowrap; }
.branch-chip > span { overflow: hidden; text-overflow: ellipsis; }
.branch-chip > svg { flex: none; }
button.branch-chip { cursor: pointer; }
button.branch-chip:hover, button.branch-chip[aria-pressed="true"] { background: var(--hover); color: var(--foreground); }
</style>
