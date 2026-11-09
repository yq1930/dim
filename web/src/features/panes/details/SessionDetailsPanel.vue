<script setup lang="ts">
import { computed, ref, watch } from "vue"
import { useI18n } from "vue-i18n"
import { Box, FileDiff, FileText, Folder, GitBranch, RefreshCw } from "lucide-vue-next"
import type { ContextPartKind, ContextUsage } from "../../../rpc/protocol"
import { useChatStore } from "../../../stores/chat"
import { useFilesStore } from "../../../stores/files"
import { useGitStore } from "../../../stores/git"
import { useModelsStore } from "../../../stores/models"
import { usePanesStore } from "../../../stores/panes"
import { useSessionsStore, workspaceName } from "../../../stores/sessions"
import { useToastStore } from "../../../stores/toast"
import { formatTokens } from "../../session/composer/usage-format"
import { sessionChanges, workspacePath } from "../changes/session-changes"

/**
 * A Session's details, as Claude Code's session panel: where it works, how its
 * context window is spent and what it has changed.
 */
const props = defineProps<{ sessionId: string }>()
const { t } = useI18n()
const chat = useChatStore()
const files = useFilesStore()
const git = useGitStore()
const models = useModelsStore()
const panes = usePanesStore()
const sessions = useSessionsStore()
const toast = useToastStore()

const view = computed(() => chat.views[props.sessionId])
const summary = computed(() => sessions.byId.get(props.sessionId))
const cwd = computed(() => summary.value?.cwd)
const branch = computed(() => cwd.value === undefined ? undefined : git.places[cwd.value]?.branch)
const readable = computed(() => view.value?.methods.includes("session.context.usage") === true)
const compactable = computed(() => view.value?.methods.includes("session.context.compact") === true)
const model = computed(() => {
  const id = summary.value?.model
  return id === undefined ? undefined : { name: models.nameOf(id), provider: models.providers?.find((provider) => provider.providerId === id.providerId)?.name }
})

const usage = ref<ContextUsage>()
const error = ref("")
const loading = ref(false)
const compacting = ref(false)

function message(cause: unknown): string {
  return cause instanceof Error && cause.message ? cause.message : String(cause)
}

async function load(): Promise<void> {
  if (!readable.value || loading.value) return
  loading.value = true
  const sessionId = props.sessionId
  try {
    const reading = await chat.usage(sessionId)
    if (sessionId !== props.sessionId) return
    usage.value = reading
    error.value = ""
  } catch (cause) {
    if (sessionId === props.sessionId) error.value = message(cause)
  } finally {
    loading.value = false
  }
}

watch(() => [props.sessionId, readable.value] as const, () => {
  usage.value = undefined
  error.value = ""
  void load()
}, { immediate: true })
// A run changes the context; read it again once the run ends.
watch(() => view.value?.running, (running, before) => { if (before === true && running === false) void load() })

const COLORS: Record<ContextPartKind, string> = {
  messages: "var(--info)",
  tools: "var(--warning)",
  mcpTools: "var(--success)",
  systemPrompt: "var(--accent-ultra)",
  instructions: "var(--danger)",
  skills: "var(--primary)",
  state: "var(--muted-foreground)",
}
const size = computed(() => usage.value?.contextWindow ?? 0)
const used = computed(() => usage.value?.estimatedInputTokens ?? 0)
const reserve = computed(() => usage.value === undefined ? 0 : Math.max(0, usage.value.contextWindow - usage.value.thresholdTokens))
const free = computed(() => usage.value === undefined ? 0 : Math.max(0, usage.value.thresholdTokens - usage.value.estimatedInputTokens))
const parts = computed(() => [...(usage.value?.parts ?? [])].sort((left, right) => right.tokens - left.tokens))

function share(tokens: number): string {
  if (size.value <= 0) return ""
  const percent = tokens / size.value * 100
  if (percent > 0 && percent < 0.1) return "<0.1%"
  return `${percent >= 10 ? Math.round(percent) : percent.toFixed(1)}%`
}

async function compact(): Promise<void> {
  if (compacting.value) return
  compacting.value = true
  try {
    const result = await chat.compact(props.sessionId)
    toast.push({ tone: result.status === "completed" ? "success" : "info", title: t(result.status === "completed" ? "details.compacted" : "details.compactSkipped") })
    await load()
  } catch (cause) {
    toast.push({ tone: "danger", title: t("details.compactFailed"), description: message(cause) })
  } finally {
    compacting.value = false
  }
}

const changed = computed(() => sessionChanges(view.value?.rows ?? [], cwd.value))
const added = computed(() => changed.value.reduce((total, file) => total + file.added, 0))
const removed = computed(() => changed.value.reduce((total, file) => total + file.removed, 0))
const CHANGED_SHOWN = 5
/** The first changed files, each named with the folder it sits in under the workspace. */
const shownChanges = computed(() => changed.value.slice(0, CHANGED_SHOWN).map((file) => {
  const shown = workspacePath(file.path, cwd.value)
  const cut = shown.lastIndexOf("/")
  return { path: file.path, name: shown.slice(cut + 1), folder: cut > 0 ? shown.slice(0, cut) : "" }
}))

function openFile(path: string): void {
  void files.open(props.sessionId, path, undefined, true).catch((cause: unknown) => {
    toast.push({ tone: "danger", title: t("chat.openFailed"), description: message(cause) })
  })
}
</script>

<template>
  <div class="session-details app-scroll-viewport">
    <section class="details-section" :aria-labelledby="`${sessionId}-environment`">
      <h3 :id="`${sessionId}-environment`">{{ t("details.environment") }}</h3>
      <p v-if="cwd" class="details-row" :title="cwd"><Folder aria-hidden="true" /><span>{{ workspaceName(cwd) }}</span><small>{{ cwd }}</small></p>
      <p v-if="branch" class="details-row"><GitBranch aria-hidden="true" /><span class="details-mono">{{ branch }}</span></p>
      <p v-if="model" class="details-row"><Box aria-hidden="true" /><span>{{ model.name }}</span><small v-if="model.provider">{{ model.provider }}</small></p>
    </section>

    <section v-if="readable" class="details-section" :aria-labelledby="`${sessionId}-context`">
      <header class="details-heading">
        <h3 :id="`${sessionId}-context`">{{ t("details.context") }}</h3>
        <button type="button" class="ui-icon-button" data-variant="ghost" data-size="xs" :disabled="loading" :aria-label="t('details.refresh')" v-tooltip="t('details.refresh')" @click="load"><RefreshCw :class="{ 'is-spinning': loading }" aria-hidden="true" /></button>
      </header>
      <p v-if="error" class="details-note" role="alert">{{ error }}</p>
      <template v-else-if="usage">
        <p class="details-total"><strong>{{ t("details.used", { percent: share(used) }) }}</strong><span>{{ formatTokens(used) }} / {{ formatTokens(size) }}</span></p>
        <div class="context-bar" role="img" :aria-label="t('details.used', { percent: share(used) })">
          <span v-for="part in parts" :key="part.kind" :style="{ width: `${part.tokens / size * 100}%`, background: COLORS[part.kind] }" />
          <span v-if="parts.length === 0" :style="{ width: `${used / size * 100}%`, background: COLORS.messages }" />
        </div>
        <ul class="context-parts">
          <li v-for="part in parts" :key="part.kind">
            <i :style="{ background: COLORS[part.kind] }" aria-hidden="true" /><span>{{ t(`details.part.${part.kind}`) }}</span><small>{{ formatTokens(part.tokens) }}</small><small>{{ share(part.tokens) }}</small>
          </li>
          <li class="is-quiet"><i aria-hidden="true" /><span>{{ t("details.reserve") }}</span><small>{{ formatTokens(reserve) }}</small><small>{{ share(reserve) }}</small></li>
          <li class="is-quiet is-free"><i aria-hidden="true" /><span>{{ t("details.free") }}</span><small>{{ formatTokens(free) }}</small><small>{{ share(free) }}</small></li>
        </ul>
        <footer class="details-compact">
          <span>{{ t("details.untilCompact", { tokens: formatTokens(free) }) }}</span>
          <button v-if="compactable" type="button" class="ui-button" data-variant="secondary" data-size="sm" :disabled="compacting || view?.running === true" @click="compact">{{ t(compacting ? "details.compacting" : "details.compact") }}</button>
        </footer>
      </template>
      <p v-else-if="!loading" class="details-note">{{ t("details.noUsage") }}</p>
    </section>

    <section v-if="changed.length > 0" class="details-section" :aria-labelledby="`${sessionId}-changes`">
      <header class="details-heading">
        <h3 :id="`${sessionId}-changes`">{{ t("details.changes") }}</h3>
        <button type="button" class="details-stat" @click="panes.show(sessionId, 'changes')"><FileDiff aria-hidden="true" /><span class="is-added">+{{ added.toLocaleString() }}</span><span class="is-removed">−{{ removed.toLocaleString() }}</span></button>
      </header>
      <button v-for="file in shownChanges" :key="file.path" type="button" class="details-row details-file" :title="file.path" @click="openFile(file.path)">
        <FileText aria-hidden="true" /><span>{{ file.name }}</span><small v-if="file.folder">{{ file.folder }}</small>
      </button>
      <button v-if="changed.length > CHANGED_SHOWN" type="button" class="details-more" @click="panes.show(sessionId, 'changes')">{{ t("details.showAll", { count: changed.length }) }}</button>
    </section>
  </div>
</template>

<style scoped>
.session-details { display: flex; min-height: 0; flex-direction: column; overflow-y: auto; padding: var(--space-3) var(--space-4) var(--space-6); }
.details-section { display: grid; gap: 2px; padding-block: var(--space-3); }
.details-section + .details-section { border-top: 1px solid var(--border); }
.details-section h3 { margin: 0 0 var(--space-2); font-size: var(--text-md); font-weight: var(--weight-semibold); }
.details-heading { display: flex; align-items: center; justify-content: space-between; gap: var(--space-2); }
.details-heading h3 { margin-bottom: var(--space-1); }
.details-row { display: flex; min-width: 0; min-height: var(--control-sm); align-items: center; gap: var(--space-3); margin: 0; padding: 0 var(--space-1); color: var(--foreground); font-size: var(--text-md); }
.details-row > svg { width: 15px; height: 15px; flex: none; color: var(--muted-foreground); }
.details-row > span { min-width: 0; flex: none; max-width: 60%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.details-row > small { min-width: 0; flex: 1; overflow: hidden; color: var(--muted-foreground); font-size: var(--text-xs); text-overflow: ellipsis; white-space: nowrap; }
.details-mono { font-family: var(--font-mono); font-size: var(--text-sm); }
.details-file { width: 100%; border: 0; border-radius: var(--control-sm-radius); background: transparent; cursor: pointer; text-align: left; }
.details-file:hover { background: var(--hover); }
.details-total { display: flex; align-items: baseline; justify-content: space-between; margin: 0 0 var(--space-2); font-size: var(--text-md); }
.details-total strong { font-weight: var(--weight-medium); }
.details-total span { color: var(--muted-foreground); font-size: var(--text-sm); font-variant-numeric: tabular-nums; }
.context-bar { display: flex; height: 6px; overflow: hidden; border-radius: 3px; background: var(--muted); margin-bottom: var(--space-3); }
.context-bar > span { height: 100%; flex: none; }
.context-bar > span + span { box-shadow: -1px 0 0 var(--background); }
.context-parts { display: grid; gap: 6px; margin: 0; padding: 0; list-style: none; }
.context-parts li { display: grid; grid-template-columns: 10px minmax(0, 1fr) auto 3.4em; align-items: center; gap: var(--space-2); font-size: var(--text-sm); }
.context-parts i { width: 10px; height: 10px; border-radius: 2px; }
.context-parts li > span { overflow: hidden; color: var(--fg-2); text-overflow: ellipsis; white-space: nowrap; }
.context-parts small { color: var(--muted-foreground); font-size: var(--text-sm); font-variant-numeric: tabular-nums; text-align: right; }
.context-parts .is-quiet i { background: var(--border-strong); }
.context-parts .is-free i { background: transparent; box-shadow: inset 0 0 0 1px var(--border-strong); }
.details-compact { display: flex; align-items: center; justify-content: space-between; gap: var(--space-2); margin-top: var(--space-3); color: var(--muted-foreground); font-size: var(--text-sm); }
.details-note { margin: 0; padding: 0 var(--space-1); color: var(--muted-foreground); font-size: var(--text-sm); }
.details-stat { display: inline-flex; align-items: center; gap: 6px; padding: 2px 6px; border: 0; border-radius: var(--control-xs-radius); background: transparent; color: var(--fg-2); font-size: var(--text-sm); font-variant-numeric: tabular-nums; cursor: pointer; }
.details-stat:hover { background: var(--hover); }
.details-stat > svg { width: 14px; height: 14px; color: var(--muted-foreground); }
.is-added { color: var(--success); }
.is-removed { color: var(--danger); }
.details-more { justify-self: start; margin-top: 2px; padding: 4px var(--space-1); border: 0; background: transparent; color: var(--muted-foreground); font-size: var(--text-sm); cursor: pointer; }
.details-more:hover { color: var(--foreground); }
</style>
