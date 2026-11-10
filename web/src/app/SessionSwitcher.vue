<script setup lang="ts">
import { computed, nextTick, ref, useId, watch } from "vue"
import { useRoute, useRouter } from "vue-router"
import { useI18n } from "vue-i18n"
import { LoaderCircle, Search } from "lucide-vue-next"
import { useSessionState } from "../features/sidebar/session-state"
import { formatRelativeTimeShort } from "../features/session/transcript/timeline-time"
import { useSessionsStore } from "../stores/sessions"
import { useUiStore } from "../stores/ui"

/**
 * Jumps to a Session, as Claude Code's ⌘K: the latest first, narrowed by what is typed of a title
 * or a workspace. The highlight starts on the first Session that is not the one on screen, so the
 * shortcut and Enter go back to the one before.
 */
const LIMIT = 50
const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const sessions = useSessionsStore()
const ui = useUiStore()
const stateOf = useSessionState()
const id = useId()
const dialog = ref<HTMLDialogElement>()
const field = ref<HTMLInputElement>()
const list = ref<HTMLElement>()
const query = ref("")
const active = ref(0)
const nowMs = ref(Date.now())

const current = computed(() => typeof route.params.sessionId === "string" ? route.params.sessionId : undefined)
const rows = computed(() => {
  const words = query.value.trim().toLocaleLowerCase().split(/\s+/).filter((word) => word !== "")
  return sessions.workspaces
    .flatMap((workspace) => workspace.sessions.map((session) => ({ session, workspace: workspace.name, search: `${session.title ?? ""} ${session.preview ?? ""} ${workspace.name} ${session.cwd}`.toLocaleLowerCase() })))
    .filter((row) => words.every((word) => row.search.includes(word)))
    .sort((left, right) => right.session.updatedAt - left.session.updatedAt)
    .slice(0, LIMIT)
})
const picked = computed(() => rows.value[Math.min(active.value, rows.value.length - 1)])

function reset(): void {
  active.value = Math.max(0, rows.value.findIndex((row) => row.session.sessionId !== current.value))
}
watch(query, reset)

watch(() => ui.switcherOpen, async (open) => {
  await nextTick()
  const element = dialog.value
  if (element === undefined) return
  if (open && !element.open) {
    query.value = ""
    nowMs.value = Date.now()
    reset()
    element.showModal()
    field.value?.focus()
  } else if (!open && element.open) element.close()
}, { immediate: true })

function move(step: 1 | -1): void {
  const count = rows.value.length
  if (count === 0) return
  active.value = (Math.min(active.value, count - 1) + step + count) % count
  void nextTick(() => list.value?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" }))
}

function go(sessionId: string): void {
  dialog.value?.close()
  ui.drawerOpen = false
  if (sessionId !== current.value) void router.push({ name: "chat-session", params: { sessionId } })
}

function onKeydown(event: KeyboardEvent): void {
  if (event.isComposing || event.keyCode === 229) return
  if (event.key === "ArrowDown" || event.key === "ArrowUp") {
    event.preventDefault()
    move(event.key === "ArrowDown" ? 1 : -1)
  } else if (event.key === "Enter") {
    event.preventDefault()
    if (picked.value !== undefined) go(picked.value.session.sessionId)
  }
}

/** A press on the backdrop, outside the dialog's box, closes it. */
function onClick(event: MouseEvent): void {
  const element = dialog.value
  if (element === undefined || event.target !== element) return
  const box = element.getBoundingClientRect()
  if (event.clientX < box.left || event.clientX > box.right || event.clientY < box.top || event.clientY > box.bottom) element.close()
}
</script>

<template>
  <dialog ref="dialog" class="ui-dialog palette session-switcher" data-size="md" :aria-label="t('switcher.title')" @close="ui.switcherOpen = false" @click="onClick">
    <div class="ui-field-wrap">
      <Search aria-hidden="true" />
      <input
        ref="field"
        v-model="query"
        class="ui-field"
        data-size="md"
        type="search"
        role="combobox"
        aria-autocomplete="list"
        aria-expanded="true"
        :aria-controls="id"
        :aria-activedescendant="picked ? `${id}-${picked.session.sessionId}` : undefined"
        :placeholder="t('switcher.title')"
        :aria-label="t('switcher.title')"
        @keydown="onKeydown"
      >
    </div>
    <div :id="id" ref="list" class="switcher-list" role="listbox" :aria-label="t('switcher.title')">
      <button
        v-for="(row, index) in rows"
        :id="`${id}-${row.session.sessionId}`"
        :key="row.session.sessionId"
        type="button"
        role="option"
        class="ui-menu-item"
        tabindex="-1"
        :aria-selected="row === picked"
        :data-highlighted="row === picked ? '' : undefined"
        @pointermove="active = index"
        @click="go(row.session.sessionId)"
      >
        <LoaderCircle v-if="stateOf(row.session) === 'running'" class="is-spinning" aria-hidden="true" />
        <span v-else class="status-dot" :data-state="stateOf(row.session)" aria-hidden="true" />
        <span>{{ row.session.title ?? row.session.preview ?? t("sidebar.untitled") }}</span>
        <small class="switcher-workspace">{{ row.workspace }}</small>
        <small class="timestamp">{{ formatRelativeTimeShort(new Date(row.session.updatedAt), nowMs) }}</small>
      </button>
      <p v-if="rows.length === 0" class="menu-empty" role="status">{{ t("switcher.none") }}</p>
    </div>
  </dialog>
</template>

<style scoped>
/* A top panel: the field stays where it opened while the list under it filters. */
.session-switcher { margin-top: 12dvh; padding: var(--space-3); }
.session-switcher[open] { display: flex; max-height: min(520px, 76dvh); flex-direction: column; }
.switcher-list { min-height: 0; flex: 1; overflow-y: auto; margin-top: var(--space-2); }
/* The highlight moves with the pointer and the arrows; a row keeps its weight under it. */
.switcher-list .ui-menu-item { font-weight: var(--weight-ui); }
.switcher-list .status-dot { margin: 0 4px; }
/* As in the sidebar, a Session at rest shows no dot. */
.switcher-list .status-dot[data-state="idle"] { visibility: hidden; }
.switcher-list small { flex: none; color: var(--muted-foreground); font-size: var(--text-xs); font-weight: var(--weight-ui); }
.switcher-workspace { max-width: 40%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
