<script setup lang="ts">
import { computed, ref, watch } from "vue"
import { useI18n } from "vue-i18n"
import { Plus, SquareTerminal, X } from "lucide-vue-next"
import { useConnectionStore } from "../../../stores/connection"
import { usePanesStore } from "../../../stores/panes"
import { useSessionsStore } from "../../../stores/sessions"
import { useTerminalsStore } from "../../../stores/terminals"
import TerminalView from "./TerminalView.vue"

/** The detail panel's terminals of a Session: one tab per shell, each opened in its workspace. */
const props = defineProps<{ sessionId: string }>()
const { t } = useI18n()
const connection = useConnectionStore()
const sessions = useSessionsStore()
const terminals = useTerminalsStore()
const panes = usePanesStore()
const list = computed(() => terminals.bySession[props.sessionId] ?? [])
const active = computed(() => terminals.active[props.sessionId])
const cwd = computed(() => sessions.byId.get(props.sessionId)?.cwd)
const opening = ref(false)
const error = ref("")
/** The Session this panel already gave a first terminal, so closing the last tab leaves it empty. */
let offered: string | undefined

function message(cause: unknown): string {
  return cause instanceof Error && cause.message ? cause.message : String(cause)
}

async function openTerminal(): Promise<void> {
  const directory = cwd.value
  if (directory === undefined || opening.value) return
  opening.value = true
  error.value = ""
  try {
    await terminals.open(props.sessionId, directory)
  } catch (cause) {
    error.value = message(cause)
  } finally {
    opening.value = false
  }
}

/** The terminal shortcuts: a new terminal, or the next or previous tab. */
watch(() => panes.request, () => {
  const command = panes.take(props.sessionId, "terminal")
  if (command === "newTerminal") void openTerminal()
  else if (command === "nextTerminal" || command === "prevTerminal") {
    const index = list.value.findIndex((terminal) => terminal.terminalId === active.value)
    const next = list.value[(index + (command === "nextTerminal" ? 1 : -1) + list.value.length) % list.value.length]
    if (next !== undefined) terminals.activate(props.sessionId, next.terminalId)
  }
}, { immediate: true })

async function closeTerminal(terminalId: string): Promise<void> {
  error.value = ""
  try {
    await terminals.close(props.sessionId, terminalId)
  } catch (cause) {
    error.value = message(cause)
  }
}

watch(() => [props.sessionId, connection.phase === "connected"] as const, async ([sessionId, ready]) => {
  if (!ready) return
  error.value = ""
  try {
    await terminals.load(sessionId)
  } catch (cause) {
    if (sessionId === props.sessionId) error.value = message(cause)
    return
  }
  if (sessionId !== props.sessionId || offered === sessionId) return
  offered = sessionId
  if (list.value.length === 0) await openTerminal()
}, { immediate: true })
</script>

<template>
  <div class="terminal-panel">
    <header class="panel-subtabs">
      <div class="panel-tablist" role="tablist" :aria-label="t('terminal.title')">
        <div v-for="(terminal, index) in list" :key="terminal.terminalId" class="panel-tab-wrap" :class="{ 'is-active': terminal.terminalId === active }">
          <button type="button" role="tab" class="panel-tab" :aria-selected="terminal.terminalId === active" :data-ended="terminal.exited" :title="terminal.cwd" @click="terminals.activate(sessionId, terminal.terminalId)">
            <SquareTerminal :size="14" aria-hidden="true" />
            <span class="panel-tab-title">{{ list.length > 1 ? `${terminal.title} ${index + 1}` : terminal.title }}</span>
          </button>
          <button type="button" class="panel-tab-close" :aria-label="t('terminal.close')" @click="closeTerminal(terminal.terminalId)"><X :size="12" aria-hidden="true" /></button>
        </div>
      </div>
      <button type="button" class="ui-icon-button" data-variant="ghost" data-size="sm" :disabled="opening || cwd === undefined || connection.phase !== 'connected'" :aria-label="t('terminal.new')" v-tooltip="t('terminal.new')" @click="openTerminal">
        <Plus aria-hidden="true" />
      </button>
    </header>
    <p v-if="error" class="ui-banner terminal-error" data-tone="danger" role="alert">{{ error }}</p>
    <TerminalView v-if="active" :key="active" :terminal-id="active" @exited="terminals.markExited(sessionId, $event)" @gone="terminals.forget(sessionId, $event)" />
    <div v-else-if="!opening" class="terminal-empty">
      <button type="button" class="ui-button" data-variant="secondary" data-size="sm" :disabled="cwd === undefined || connection.phase !== 'connected'" @click="openTerminal"><Plus aria-hidden="true" />{{ t("terminal.new") }}</button>
    </div>
  </div>
</template>

<style scoped>
.terminal-panel { display: flex; min-width: 0; min-height: 0; flex-direction: column; background: var(--background); }
.terminal-error { margin: var(--space-2) var(--space-3) 0; }
.terminal-empty { display: flex; flex: 1; align-items: center; justify-content: center; }
</style>
