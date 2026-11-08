<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue"
import { useI18n } from "vue-i18n"
import { ExternalLink, LayoutList, LoaderCircle, Play, RefreshCw, ScrollText, Sparkles, Square } from "lucide-vue-next"
import { MAIN_BRANCH } from "../../rpc/protocol"
import { useChatStore } from "../../stores/chat"
import { useConnectionStore } from "../../stores/connection"
import { localUrl, usePreviewsStore, type LaunchConfig } from "../../stores/previews"
import { useSessionsStore } from "../../stores/sessions"
import { useToastStore } from "../../stores/toast"

/**
 * The detail panel's preview of a Session: a dev server on this computer in a
 * frame, as Claude Code's Preview pane shows one. The servers come from the
 * project's `.claude/launch.json` and run in terminals of the Session; their
 * output shows below the page. Any address on this computer can be opened too.
 */
const props = defineProps<{ sessionId: string }>()
const { t } = useI18n()
const chat = useChatStore()
const connection = useConnectionStore()
const previews = usePreviewsStore()
const sessions = useSessionsStore()
const toast = useToastStore()
const logView = ref<HTMLElement>()
/** The launch file the agent is asked to write, in Claude Code's format. */
const LAUNCH_EXAMPLE = JSON.stringify({ version: "0.0.1", configurations: [{ name: "web", runtimeExecutable: "npm", runtimeArgs: ["run", "dev"], port: 5173 }] })
const address = ref("")
const busy = ref<string>()

const preview = computed(() => previews.previews[props.sessionId] ?? { reload: 0 })
const servers = computed(() => previews.servers[props.sessionId] ?? [])
const shown = computed(() => servers.value.find((server) => server.terminalId === preview.value.serverId))
const configs = computed(() => previews.configs[props.sessionId])
const cwd = computed(() => sessions.byId.get(props.sessionId)?.cwd)
const startable = computed(() => connection.supports("terminal.open") && cwd.value !== undefined)

function failure(cause: unknown): string {
  return cause instanceof Error && cause.message ? cause.message : String(cause)
}

/** The running server a launch entry started, if any. */
function running(config: LaunchConfig) {
  return servers.value.find((server) => server.name === config.name && !server.exited)
}

async function go(): Promise<void> {
  const url = localUrl(address.value)
  if (url === undefined) {
    toast.push({ tone: "danger", title: t("preview.notLocal") })
    return
  }
  await previews.show(props.sessionId, url)
}

async function start(config: LaunchConfig): Promise<void> {
  if (cwd.value === undefined || busy.value !== undefined) return
  busy.value = config.name
  try {
    await previews.start(props.sessionId, cwd.value, config)
  } catch (cause) {
    toast.push({ tone: "danger", title: t("preview.startFailed", { name: config.name }), description: failure(cause) })
  } finally {
    busy.value = undefined
  }
}

async function stop(terminalId: string): Promise<void> {
  try {
    await previews.stop(props.sessionId, terminalId)
  } catch (cause) {
    toast.push({ tone: "danger", title: t("preview.stopFailed"), description: failure(cause) })
  }
}

async function restart(): Promise<void> {
  const server = shown.value
  const config = configs.value?.list?.find((entry) => entry.name === server?.name)
  if (server === undefined || config === undefined) return
  previews.dismiss(props.sessionId, server.terminalId)
  await start(config)
}

async function openServer(terminalId: string): Promise<void> {
  const server = servers.value.find((entry) => entry.terminalId === terminalId)
  if (server?.url !== undefined) await previews.show(props.sessionId, server.url, terminalId)
}

/** Asks the main line to find the project's dev server and declare it, as Claude Code's "Detect dev server" does. */
async function detect(): Promise<void> {
  try {
    await chat.submit(props.sessionId, MAIN_BRANCH, t("preview.detectPrompt", { format: LAUNCH_EXAMPLE }))
    toast.push({ tone: "info", title: t("preview.detectSent") })
  } catch (cause) {
    toast.push({ tone: "danger", title: t("preview.detectFailed"), description: failure(cause) })
  }
}

function openExternal(): void {
  if (preview.value.url !== undefined) window.open(preview.value.url, "_blank", "noopener")
}

watch(() => props.sessionId, (sessionId) => {
  if (previews.configs[sessionId] === undefined) void previews.loadConfigs(sessionId)
  void previews.discover(sessionId).then(() => previews.restore(sessionId)).catch(() => undefined)
}, { immediate: true })
watch(() => preview.value.url, (url) => { address.value = url ?? "" }, { immediate: true })
// The logs follow the end of the output while the reader is there.
watch(() => shown.value?.logs, async () => {
  const element = logView.value
  if (element === undefined) return
  const atEnd = element.scrollHeight - element.scrollTop - element.clientHeight < 24
  await nextTick()
  if (atEnd) element.scrollTop = element.scrollHeight
})
</script>

<template>
  <div class="preview-panel">
    <form class="preview-bar" @submit.prevent="go">
      <button v-if="preview.url || preview.serverId" type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('preview.servers')" v-tooltip="t('preview.servers')" @click="previews.leave(sessionId)"><LayoutList aria-hidden="true" /></button>
      <input v-model="address" class="ui-field preview-address" data-size="sm" spellcheck="false" autocomplete="off" :placeholder="t('preview.addressPlaceholder')" :aria-label="t('preview.address')" />
      <template v-if="preview.url">
        <button type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('preview.reload')" v-tooltip="t('preview.reload')" @click="previews.reload(sessionId)"><RefreshCw aria-hidden="true" /></button>
        <button type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('preview.openExternal')" v-tooltip="t('preview.openExternal')" @click="openExternal"><ExternalLink aria-hidden="true" /></button>
      </template>
      <template v-if="shown">
        <button type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-pressed="previews.logsOpen" :aria-label="previews.logsOpen ? t('preview.hideLogs') : t('preview.showLogs')" v-tooltip="previews.logsOpen ? t('preview.hideLogs') : t('preview.showLogs')" @click="previews.logsOpen = !previews.logsOpen"><ScrollText aria-hidden="true" /></button>
        <button v-if="!shown.exited" type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('preview.stop')" v-tooltip="t('preview.stop')" @click="stop(shown.terminalId)"><Square aria-hidden="true" /></button>
      </template>
    </form>
    <div class="preview-body">
      <iframe v-if="preview.url" :key="`${preview.url}#${preview.reload}`" class="preview-frame" :src="preview.url" :title="t('preview.title')" sandbox="allow-scripts allow-same-origin allow-forms allow-popups allow-modals allow-downloads" />
      <div v-if="shown && shown.exited" class="preview-cover">
        <p>{{ t("preview.stopped") }}<template v-if="shown.exitCode !== undefined"> {{ t("preview.exitCode", { code: shown.exitCode }) }}</template></p>
        <span class="preview-cover-actions">
          <button v-if="configs?.list?.some((entry) => entry.name === shown!.name)" type="button" class="ui-button" data-variant="primary" data-size="sm" @click="restart"><Play aria-hidden="true" />{{ t("preview.restart") }}</button>
          <button v-if="!previews.logsOpen" type="button" class="ui-button" data-variant="secondary" data-size="sm" @click="previews.logsOpen = true"><ScrollText aria-hidden="true" />{{ t("preview.showLogs") }}</button>
        </span>
      </div>
      <div v-else-if="shown && !shown.ready" class="preview-cover" role="status"><LoaderCircle class="is-spinning" aria-hidden="true" />{{ t("preview.starting", { name: shown.name }) }}</div>
      <section v-if="!preview.url && !shown" class="preview-home">
        <h3 class="preview-heading">{{ t("preview.servers") }}</h3>
        <p v-if="configs?.error" class="ui-banner" data-tone="danger" role="alert">{{ t("preview.launchError", { error: configs.error }) }}</p>
        <ul v-else-if="configs?.list?.length" class="preview-servers">
          <li v-for="config in configs.list" :key="config.name" class="preview-server">
            <span class="preview-server-main">
              <strong><span v-if="running(config)" class="status-dot" data-state="running" aria-hidden="true" />{{ config.name }}</strong>
              <small :title="config.command.join(' ')">{{ config.command.join(" ") }}<template v-if="config.port"> · {{ config.port }}</template></small>
            </span>
            <template v-if="running(config)">
              <button type="button" class="ui-button" data-variant="secondary" data-size="sm" :disabled="running(config)!.url === undefined" @click="openServer(running(config)!.terminalId)">{{ t("preview.open") }}</button>
              <button type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :aria-label="t('preview.stop')" v-tooltip="t('preview.stop')" @click="stop(running(config)!.terminalId)"><Square aria-hidden="true" /></button>
            </template>
            <button v-else type="button" class="ui-button" data-variant="primary" data-size="sm" :disabled="!startable || busy !== undefined" @click="start(config)"><Play aria-hidden="true" />{{ t("preview.start") }}</button>
          </li>
        </ul>
        <div v-else-if="configs" class="preview-empty">
          <p>{{ t("preview.noServers") }}</p>
          <button v-if="chat.views[sessionId]?.status === 'ready'" type="button" class="ui-button" data-variant="secondary" data-size="sm" @click="detect"><Sparkles aria-hidden="true" />{{ t("preview.detect") }}</button>
        </div>
      </section>
    </div>
    <pre v-if="shown && previews.logsOpen" ref="logView" class="preview-logs">{{ shown.logs || t("preview.noLogs") }}</pre>
  </div>
</template>

<style scoped>
.preview-panel { display: flex; min-width: 0; min-height: 0; flex-direction: column; background: var(--background); }
.preview-bar { display: flex; min-width: 0; height: 40px; flex: none; align-items: center; gap: var(--space-1); padding: 0 var(--space-2); border-bottom: 1px solid var(--border); }
.preview-address { min-width: 0; flex: 1; font-family: var(--font-mono); font-size: var(--text-xs); }
.preview-body { position: relative; display: flex; min-height: 0; flex: 1; flex-direction: column; }
.preview-frame { width: 100%; min-height: 0; flex: 1; border: 0; background: #fff; }
.preview-cover { position: absolute; inset: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: var(--space-3); padding: var(--space-4); background: var(--background); color: var(--muted-foreground); font-size: var(--text-sm); text-align: center; }
.preview-cover > p { margin: 0; }
.preview-cover > svg { width: 18px; height: 18px; }
.preview-cover-actions { display: inline-flex; gap: var(--space-2); }
.preview-home { min-height: 0; flex: 1; overflow: auto; padding: var(--space-4); }
.preview-heading { margin: 0 0 var(--space-3); color: var(--muted-foreground); font-size: var(--text-xs); font-weight: var(--weight-medium); }
.preview-servers { display: grid; gap: var(--space-2); margin: 0; padding: 0; list-style: none; }
.preview-server { display: flex; align-items: center; gap: var(--space-2); padding: var(--space-2) var(--space-2) var(--space-2) var(--space-3); border: 1px solid var(--border); border-radius: var(--radius-lg); background: var(--card); }
.preview-server-main { display: grid; min-width: 0; flex: 1; gap: 2px; }
.preview-server-main strong { display: inline-flex; align-items: center; gap: 6px; font-size: var(--text-sm); font-weight: var(--weight-medium); }
.preview-server-main small { overflow: hidden; color: var(--muted-foreground); font-family: var(--font-mono); font-size: var(--text-xs); text-overflow: ellipsis; white-space: nowrap; }
.preview-empty { display: grid; justify-items: start; gap: var(--space-3); color: var(--muted-foreground); font-size: var(--text-sm); }
.preview-empty > p { margin: 0; }
.preview-logs { height: 38%; min-height: 96px; flex: none; margin: 0; overflow: auto; padding: var(--space-2) var(--space-3); border-top: 1px solid var(--border); background: var(--muted); color: color-mix(in oklab, var(--foreground) 85%, transparent); font: var(--text-xs)/1.5 var(--font-mono); white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
