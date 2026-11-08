<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue"
import { useRouter } from "vue-router"
import { useI18n } from "vue-i18n"
import { CircleCheck, CircleDashed, CircleMinus, CircleX, Copy, ExternalLink, GitFork, GitPullRequest, LoaderCircle, RefreshCw } from "lucide-vue-next"
import ConfirmDialog from "../../components/ConfirmDialog.vue"
import { copyText } from "../../clipboard"
import { MAIN_BRANCH } from "../../rpc/protocol"
import type { GitCheckState, GitPullRequestView, GitStatus } from "../../rpc/git"
import { useChatStore } from "../../stores/chat"
import { useConnectionStore } from "../../stores/connection"
import { useGitStore } from "../../stores/git"
import { useSessionsStore } from "../../stores/sessions"
import { useToastStore } from "../../stores/toast"
import { usePanesStore } from "../../stores/panes"
import { useUiStore } from "../../stores/ui"
import { notePullRequest } from "./auto-archive"

/**
 * The detail panel's Git view of a Session's directory: its branch and how
 * it stands against its upstream, the branch's pull request with its review
 * and checks, and, for a worktree the Web workspace made, its removal.
 */
const props = defineProps<{ sessionId: string }>()
const { t } = useI18n()
const router = useRouter()
const chat = useChatStore()
const connection = useConnectionStore()
const git = useGitStore()
const sessions = useSessionsStore()
const toast = useToastStore()
const ui = useUiStore()
const confirm = ref<InstanceType<typeof ConfirmDialog>>()
const status = ref<GitStatus>()
const pullRequest = ref<GitPullRequestView>()
const error = ref("")
const loading = ref(false)
const removing = ref(false)
const cwd = computed(() => sessions.byId.get(props.sessionId)?.cwd)
const view = computed(() => chat.views[props.sessionId])
const running = computed(() => view.value?.sessionRunning === true || view.value?.running === true)
const worktree = computed(() => status.value !== undefined && status.value.root !== status.value.main)
const checks = computed(() => pullRequest.value?.available === true ? pullRequest.value.pullRequest?.checks ?? [] : [])
const tally = computed(() => (["failure", "pending", "success", "skipped"] as const).flatMap((state) => {
  const count = checks.value.filter((check) => check.state === state || (state === "skipped" && check.state === "neutral")).length
  return count === 0 ? [] : [t(`git.checks.${state}`, { count })]
}).join(" · "))
const REFRESH_MS = 60_000
let generation = 0
let timer: ReturnType<typeof setInterval> | undefined

function message(cause: unknown): string {
  return cause instanceof Error && cause.message ? cause.message : String(cause)
}

async function refresh(): Promise<void> {
  const directory = cwd.value
  if (directory === undefined || connection.phase !== "connected") return
  const request = ++generation
  loading.value = true
  const [found, pr] = await Promise.allSettled([git.status(directory), git.pullRequest(directory)])
  if (request !== generation) return
  loading.value = false
  if (found.status === "fulfilled") { status.value = found.value; error.value = "" } else error.value = message(found.reason)
  if (pr.status === "fulfilled") {
    pullRequest.value = pr.value
    if (pr.value.available) notePullRequest(directory, pr.value.pullRequest)
  }
}

watch([cwd, () => connection.phase], () => {
  status.value = undefined
  pullRequest.value = undefined
  void refresh()
}, { immediate: true })
// A run may commit, push or open the pull request; read again once it ends.
watch(running, (now, before) => { if (before && !now) void refresh() })
timer = setInterval(() => { void refresh() }, REFRESH_MS)
onBeforeUnmount(() => {
  clearInterval(timer)
  ++generation
})

const STATE_ICON: Record<GitCheckState, typeof CircleCheck> = { success: CircleCheck, failure: CircleX, pending: CircleDashed, skipped: CircleMinus, neutral: CircleMinus }

function prTone(state: string, draft: boolean): string | undefined {
  if (draft) return undefined
  return state === "open" ? "primary" : state === "merged" ? "success" : undefined
}

async function copyPath(path: string): Promise<void> {
  try {
    await copyText(path)
    toast.push({ tone: "success", title: t("git.copied") })
  } catch (cause) {
    toast.push({ tone: "danger", title: message(cause) })
  }
}

async function createPullRequest(): Promise<void> {
  try {
    await chat.submit(props.sessionId, MAIN_BRANCH, t("git.createPrPrompt"))
  } catch (cause) {
    toast.push({ tone: "danger", title: t("command.failed"), description: message(cause) })
  }
}

/** Removes the worktree the Session runs in, then archives the Session and opens a new one in the main repository. */
async function removeWorktree(): Promise<void> {
  const found = status.value
  if (found === undefined || removing.value || running.value) return
  if (!await confirm.value?.open(t("git.removeConfirm", { path: found.root }), { confirmLabel: t("git.remove") })) return
  removing.value = true
  try {
    await git.removeWorktree(found.root)
    sessions.forgetWorkspace(found.root)
    if (connection.supports("session.archive")) await sessions.setArchived(props.sessionId, true)
    if (ui.lastSessionId === props.sessionId) ui.lastSessionId = undefined
    usePanesStore().close(props.sessionId)
    await router.replace({ name: "chat-new", query: { cwd: found.main } })
  } catch (cause) {
    error.value = message(cause)
  } finally {
    removing.value = false
  }
}
</script>

<template>
  <div class="git-panel app-scroll-viewport" role="region" :aria-label="t('git.tab')">
    <p v-if="error" class="ui-banner" data-tone="danger" role="alert">{{ error }}</p>
    <p v-if="!status && loading" class="panel-empty" role="status">{{ t("common.loading") }}</p>

    <section v-if="status" class="git-card">
      <header class="git-card-header">
        <GitFork :size="14" aria-hidden="true" />
        <strong class="git-branch" :title="status.branch">{{ status.branch ?? t("git.detached") }}</strong>
        <span v-if="worktree" class="ui-badge" data-size="sm">{{ t("git.worktree") }}</span>
        <button type="button" class="ui-icon-button git-refresh" data-variant="ghost" data-size="xs" :disabled="loading" :aria-label="t('git.refresh')" v-tooltip="t('git.refresh')" @click="refresh">
          <RefreshCw :class="{ 'is-spinning': loading }" aria-hidden="true" />
        </button>
      </header>
      <dl class="git-facts">
        <div>
          <dt>{{ t("git.upstream") }}</dt>
          <dd v-if="status.upstream"><code>{{ status.upstream }}</code><span class="git-ahead">↑{{ status.ahead }} ↓{{ status.behind }}</span></dd>
          <dd v-else>{{ t("git.noUpstream") }}</dd>
        </div>
        <div>
          <dt>{{ t("git.changes") }}</dt>
          <dd>{{ status.changed === 0 ? t("git.clean") : t("git.changed", { count: status.changed }, status.changed) }}</dd>
        </div>
        <div v-if="worktree">
          <dt>{{ t("git.location") }}</dt>
          <dd class="git-path"><code :title="status.root">{{ status.root }}</code><button type="button" class="ui-icon-button" data-variant="ghost" data-size="xs" :aria-label="t('git.copyPath')" v-tooltip="t('git.copyPath')" @click="copyPath(status.root)"><Copy aria-hidden="true" /></button></dd>
        </div>
        <div v-if="worktree">
          <dt>{{ t("git.mainRepository") }}</dt>
          <dd class="git-path"><code :title="status.main">{{ status.main }}</code></dd>
        </div>
      </dl>
    </section>

    <section v-if="status" class="git-card">
      <header class="git-card-header">
        <GitPullRequest :size="14" aria-hidden="true" />
        <strong>{{ t("git.pullRequest") }}</strong>
      </header>
      <p v-if="pullRequest === undefined" class="git-note" role="status">{{ t("common.loading") }}</p>
      <p v-else-if="!pullRequest.available" class="git-note">{{ pullRequest.reason === "failed" ? pullRequest.message : t(`git.unavailable.${pullRequest.reason}`) }}</p>
      <template v-else-if="pullRequest.pullRequest">
        <a class="git-pr-title" :href="pullRequest.pullRequest.url" target="_blank" rel="noopener noreferrer">
          <span>#{{ pullRequest.pullRequest.number }} {{ pullRequest.pullRequest.title }}</span><ExternalLink :size="12" aria-hidden="true" />
        </a>
        <div class="git-pr-meta">
          <span class="ui-badge" data-size="sm" :data-tone="prTone(pullRequest.pullRequest.state, pullRequest.pullRequest.draft)">{{ pullRequest.pullRequest.draft && pullRequest.pullRequest.state === "open" ? t("git.prState.draft") : t(`git.prState.${pullRequest.pullRequest.state}`) }}</span>
          <span v-if="pullRequest.pullRequest.review" class="ui-badge" data-size="sm" :data-tone="pullRequest.pullRequest.review === 'approved' ? 'success' : pullRequest.pullRequest.review === 'changes_requested' ? 'warning' : undefined">{{ t(`git.review.${pullRequest.pullRequest.review}`) }}</span>
          <code class="git-refs">{{ pullRequest.pullRequest.base }} ← {{ pullRequest.pullRequest.head }}</code>
        </div>
        <p class="git-note">{{ checks.length === 0 ? t("git.noChecks") : tally }}</p>
        <ul v-if="checks.length" class="git-checks">
          <li v-for="(check, index) in checks" :key="index" :data-state="check.state">
            <LoaderCircle v-if="check.state === 'pending'" :size="14" class="is-spinning" aria-hidden="true" />
            <component :is="STATE_ICON[check.state]" v-else :size="14" aria-hidden="true" />
            <a v-if="check.url" :href="check.url" target="_blank" rel="noopener noreferrer">{{ check.name }}</a>
            <span v-else>{{ check.name }}</span>
          </li>
        </ul>
      </template>
      <template v-else>
        <p class="git-note">{{ t("git.noPullRequest") }}</p>
        <button v-if="status.branch" type="button" class="ui-button git-action" data-variant="secondary" data-size="sm" :disabled="running || connection.phase !== 'connected'" @click="createPullRequest">{{ t("git.createPr") }}</button>
      </template>
    </section>

    <section v-if="status?.managed" class="git-card">
      <button type="button" class="ui-button git-action" data-variant="danger" data-size="sm" :disabled="removing || running || connection.phase !== 'connected'" @click="removeWorktree">{{ t("git.removeWorktree") }}</button>
    </section>
    <ConfirmDialog ref="confirm" />
  </div>
</template>

<style scoped>
.git-panel { display: flex; min-width: 0; min-height: 0; flex-direction: column; gap: var(--space-3); padding: var(--space-3); overflow-y: auto; font-size: var(--text-sm); }
.git-card { display: flex; flex: none; flex-direction: column; gap: var(--space-2); padding: var(--space-3); border: 1px solid var(--border); border-radius: var(--radius-lg); background: var(--card); }
.git-card-header { display: flex; min-width: 0; align-items: center; gap: var(--space-2); }
.git-card-header > svg { flex: none; color: var(--muted-foreground); }
.git-branch { min-width: 0; overflow: hidden; font-family: var(--font-mono); text-overflow: ellipsis; white-space: nowrap; }
.git-refresh { margin-left: auto; }
.git-facts { display: grid; gap: var(--space-1); margin: 0; }
.git-facts > div { display: grid; grid-template-columns: 5em minmax(0, 1fr); gap: var(--space-2); align-items: baseline; }
.git-facts dt { color: var(--muted-foreground); }
.git-facts dd { display: flex; min-width: 0; align-items: center; gap: var(--space-2); margin: 0; }
.git-facts code, .git-refs { min-width: 0; overflow: hidden; font-family: var(--font-mono); font-size: var(--text-xs); text-overflow: ellipsis; white-space: nowrap; }
.git-ahead { flex: none; color: var(--muted-foreground); font-family: var(--font-mono); font-size: var(--text-xs); }
.git-note { margin: 0; color: var(--muted-foreground); }
.git-pr-title { display: flex; min-width: 0; align-items: center; gap: var(--space-1); color: var(--foreground); font-weight: var(--weight-medium); text-decoration: none; }
.git-pr-title > span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.git-pr-title:hover > span { text-decoration: underline; }
.git-pr-title > svg { flex: none; color: var(--muted-foreground); }
.git-pr-meta { display: flex; min-width: 0; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
.git-refs { color: var(--muted-foreground); }
.git-checks { display: grid; gap: var(--space-1); margin: 0; padding: 0; list-style: none; }
.git-checks li { display: flex; min-width: 0; align-items: center; gap: var(--space-2); }
.git-checks li > svg { flex: none; color: var(--muted-foreground); }
.git-checks li[data-state="success"] > svg { color: var(--success); }
.git-checks li[data-state="failure"] > svg { color: var(--danger); }
.git-checks li[data-state="pending"] > svg { color: var(--warning); }
.git-checks a, .git-checks span { min-width: 0; overflow: hidden; color: var(--foreground); text-overflow: ellipsis; white-space: nowrap; }
.git-checks a:hover { text-decoration: underline; }
.git-action { align-self: flex-start; }
</style>
