<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue"
import { RouterView, useRoute, useRouter } from "vue-router"
import { useI18n } from "vue-i18n"
import { Menu, PanelLeftOpen, X } from "lucide-vue-next"
import AppSidebar from "../components/AppSidebar.vue"
import DetailPanel from "../components/DetailPanel.vue"
import ToastRegion from "../components/ToastRegion.vue"
import ChatPage from "../features/chat/ChatPage.vue"
import { useAutoArchive } from "../features/git/auto-archive"
import EndLineDialog from "../features/lines/EndLineDialog.vue"
import NewLineDialog from "../features/lines/NewLineDialog.vue"
import { overlayTitleBar } from "../desktop"
import { settingsMessages } from "../i18n/settings"
import { useConnectionStore } from "../stores/connection"
import { useFilesStore } from "../stores/files"
import { usePreferencesStore } from "../stores/preferences"
import { useSessionsStore, workspaceName } from "../stores/sessions"
import { useTerminalsStore } from "../stores/terminals"
import { useToastStore } from "../stores/toast"
import { useUiStore, type DetailView, type Pane } from "../stores/ui"
import { useKeepAwake } from "./keep-awake"
import ShortcutsDialog from "./ShortcutsDialog.vue"
import { matchShortcut, shortcutContext, withKeys, type ShortcutCommand } from "./shortcuts"

const route = useRoute()
const router = useRouter()
const { t } = useI18n()
const { t: settingsText } = useI18n({ messages: settingsMessages })
const connection = useConnectionStore()
const sessions = useSessionsStore()
const ui = useUiStore()
const terminals = useTerminalsStore()
const files = useFilesStore()
const root = ref<HTMLElement>()
const drawer = ref<HTMLDialogElement>()
let observer: ResizeObserver | undefined

const settingsRoute = computed(() => route.path === "/settings" || route.path.startsWith("/settings/"))
/** Pages that draw their own header in place of the generic one. */
const chatRoute = computed(() => route.name === "chat-new" || route.name === "chat-session" || route.name === "schedules")
/** Pages without a top bar of their own get an empty one to drag the window by. */
const titleBarStrip = computed(() => overlayTitleBar && (settingsRoute.value || route.name === "chat-new"))
const sessionId = computed(() => typeof route.params.sessionId === "string" ? route.params.sessionId : undefined)
const detailOpen = computed(() => ui.detailOpen && route.name === "chat-session" && sessionId.value !== undefined)
const splitId = computed(() => {
  const id = ui.splitSessionId
  return route.name === "chat-session" && id !== undefined && id !== sessionId.value && ui.splitAvailable && sessions.byId.has(id) ? id : undefined
})
watch(splitId, (id) => { if (id === undefined) ui.activePane = "main" })
useKeepAwake()
useAutoArchive(() => [sessionId.value, splitId.value])
const pageTitle = computed(() => {
  if (settingsRoute.value) return settingsText("settings")
  if (route.name === "chat-session" && typeof route.params.sessionId === "string") {
    const session = sessions.byId.get(route.params.sessionId)
    return session?.title ?? session?.preview ?? (session === undefined ? t("route.session") : workspaceName(session.cwd))
  }
  if (route.name === "schedules") return t("schedules.title")
  if (route.name === "not-found") return t("route.notFound")
  return t("route.newSession")
})

watch(pageTitle, (title) => { document.title = `${title} — ${t("brand.product")}` }, { immediate: true })
watch(() => ui.drawerOpen, async (open) => {
  await nextTick()
  const element = drawer.value
  if (element === undefined) return
  if (open && !element.open) element.showModal()
  else if (!open && element.open) element.close()
})

/**
 * Menus and popovers are native `<details>`, which only close from their own
 * summary; a press anywhere else or Escape dismisses them.
 */
const POPOVERS = "details.session-menu[open], details.composer-popover[open]"

function closeMenusOutside(event: PointerEvent): void {
  const target = event.target instanceof Node ? event.target : undefined
  for (const menu of document.querySelectorAll<HTMLDetailsElement>(POPOVERS)) {
    if (target === undefined || !menu.contains(target)) menu.open = false
  }
}

/** The last "Switched to … view" toast, replaced by the next. */
let viewToast: number | undefined

/** The pane the operator is in, of those shown. */
const activePane = computed<Pane>(() => ui.activePane === "split" && splitId.value !== undefined ? "split" : "main")

/** Steps the Session in the active pane through the transcript views. */
function cycleTranscript(): boolean {
  const target = route.name !== "chat-session" ? undefined : activePane.value === "split" ? splitId.value : sessionId.value
  if (target === undefined) return false
  const view = usePreferencesStore().cycleView(target)
  const toast = useToastStore()
  if (viewToast !== undefined) toast.dismiss(viewToast)
  viewToast = toast.push({ tone: "info", title: t("transcript.switched", { view: t(`transcript.${view}`) }) })
  return true
}

/** Shows or hides one view of the right panel, where the Session has it. */
function togglePane(view: DetailView): boolean {
  const id = sessionId.value
  if (route.name !== "chat-session" || id === undefined) return false
  if ((view === "files" && !files.readable(id)) || (view === "terminal" && !terminals.available)) return false
  ui.toggleDetail(view)
  return true
}

/** Closes the right panel, or the pane beside the routed Session. */
function closePane(): boolean {
  if (detailOpen.value) ui.closeDetail()
  else if (splitId.value !== undefined) ui.closeSplit()
  else return false
  return true
}

function openComposerMenu(menu: "mode" | "model"): boolean {
  if (route.name !== "chat-new" && route.name !== "chat-session") return false
  ui.requestComposerMenu(activePane.value, menu)
  return true
}

/** What each command of the shortcut table does here; false when it does nothing on this page. */
const commands: Record<ShortcutCommand, () => boolean> = {
  showShortcuts: () => {
    ui.shortcutsOpen = !ui.shortcutsOpen
    return true
  },
  toggleSidebar: () => {
    if (ui.shellMode === "compact") ui.drawerOpen = !ui.drawerOpen
    else ui.sidebarCollapsed = !ui.sidebarCollapsed
    return true
  },
  newSession: () => {
    const cwd = sessionId.value === undefined ? undefined : sessions.byId.get(sessionId.value)?.cwd
    void router.push({ name: "chat-new", ...(cwd === undefined ? {} : { query: { cwd } }) })
    return true
  },
  openSettings: () => {
    if (!settingsRoute.value) void router.push("/settings")
    return true
  },
  cycleTranscriptMode: cycleTranscript,
  toggleDiff: () => togglePane("changes"),
  toggleFiles: () => togglePane("files"),
  togglePreview: () => togglePane("preview"),
  toggleTerminal: () => togglePane("terminal"),
  closePane,
  toggleExpandPane: () => {
    if (!detailOpen.value) return false
    ui.detailExpanded = !ui.detailExpanded
    return true
  },
  openModeMenu: () => openComposerMenu("mode"),
  openModelMenu: () => openComposerMenu("model"),
}
const keys = shortcutContext()

function onKey(event: KeyboardEvent): void {
  if (event.key === "Escape") {
    for (const menu of document.querySelectorAll<HTMLDetailsElement>(POPOVERS)) menu.open = false
    return
  }
  if (event.defaultPrevented || event.repeat || event.isComposing) return
  const command = matchShortcut(event, keys)
  if (command === undefined) return
  // An open dialog keeps the keyboard; the shortcuts dialog still closes with its own keys.
  const dialog = document.querySelector("dialog[open]:not(.drawer-content)")
  if (dialog !== null && !(command === "showShortcuts" && dialog.classList.contains("shortcuts-dialog"))) return
  if (commands[command]()) event.preventDefault()
}

/** The directory has no change notification; coming back to the tab refreshes it. */
function onFocus(): void {
  if (connection.phase === "connected") void sessions.refresh()
}

onMounted(() => {
  if (root.value !== undefined) {
    observer = new ResizeObserver(([entry]) => {
      if (entry !== undefined) ui.setShellWidth(entry.contentRect.width)
    })
    observer.observe(root.value)
  }
  window.addEventListener("keydown", onKey)
  window.addEventListener("focus", onFocus)
  document.addEventListener("pointerdown", closeMenusOutside, true)
})

onBeforeUnmount(() => {
  observer?.disconnect()
  window.removeEventListener("keydown", onKey)
  window.removeEventListener("focus", onFocus)
  document.removeEventListener("pointerdown", closeMenusOutside, true)
})

function closeDrawerFromBackdrop(event: MouseEvent): void {
  if (event.target === drawer.value) ui.drawerOpen = false
}

function startResize(event: PointerEvent): void {
  if (ui.shellMode === "compact") return
  const startX = event.clientX
  const startWidth = ui.detailWidth
  const move = (next: PointerEvent): void => {
    ui.setDetailWidth(startWidth + startX - next.clientX)
  }
  const stop = (): void => {
    window.removeEventListener("pointermove", move)
    window.removeEventListener("pointerup", stop)
  }
  window.addEventListener("pointermove", move)
  window.addEventListener("pointerup", stop, { once: true })
}

function resizeBy(delta: number): void {
  ui.setDetailWidth(ui.detailWidth + delta)
}

function focusMain(): void {
  ui.detailFocused = false
  ui.activePane = "main"
}
</script>

<template>
  <div
    ref="root"
    class="app-shell"
    :data-mode="ui.shellMode"
    :data-sidebar-collapsed="ui.sidebarCollapsed"
    :data-chat-route="chatRoute"
    :data-settings-route="settingsRoute"
    style="--sidebar-width: 280px"
  >
    <ToastRegion />

    <header v-if="ui.shellMode === 'compact' && !settingsRoute" class="compact-header" data-tauri-drag-region="deep">
      <button class="ui-icon-button" data-variant="ghost" data-size="md" type="button" :aria-label="t('sidebar.openMenu')" @click="ui.drawerOpen = true">
        <Menu aria-hidden="true" />
      </button>
      <span class="compact-title">{{ pageTitle }}</span>
    </header>

    <dialog v-if="ui.shellMode === 'compact'" ref="drawer" class="drawer-content" @click="closeDrawerFromBackdrop" @close="ui.drawerOpen = false">
      <h2 class="sr-only">{{ t("sidebar.workspaces") }}</h2>
      <button autofocus class="ui-icon-button drawer-close" data-variant="ghost" data-size="md" type="button" :aria-label="t('sidebar.closeMenu')" @click="ui.drawerOpen = false">
        <X aria-hidden="true" />
      </button>
      <AppSidebar @navigate="ui.drawerOpen = false" />
    </dialog>

    <AppSidebar v-if="ui.shellMode !== 'compact' && !ui.sidebarCollapsed && !settingsRoute" class="inline-sidebar" />

    <div
      class="content-region"
      :data-has-detail="detailOpen"
      :data-detail-expanded="ui.detailExpanded"
      :data-split="splitId !== undefined"
      :style="{ '--detail-width': `${ui.detailWidth}px` }"
    >
      <main id="main" class="main-region" :aria-label="t('a11y.mainContent')" :data-active="splitId !== undefined && ui.activePane === 'main'" @pointerdown="focusMain" @focusin="focusMain">
        <button v-if="ui.sidebarCollapsed && ui.shellMode !== 'compact' && !settingsRoute" class="ui-icon-button sidebar-reopen" data-variant="ghost" data-size="md" type="button" :aria-label="t('sidebar.openMenu')" v-tooltip="withKeys(t('sidebar.openMenu'), 'toggleSidebar')" @click="ui.sidebarCollapsed = false">
          <PanelLeftOpen aria-hidden="true" />
        </button>
        <div v-if="titleBarStrip" class="window-titlebar" data-tauri-drag-region="deep" />
        <header v-if="!settingsRoute" class="main-header" data-tauri-drag-region="deep">
          <div><p>{{ t("brand.product") }} {{ connection.hello?.serverVersion }}</p><strong>{{ pageTitle }}</strong></div>
        </header>
        <p v-if="connection.phase === 'reconnecting'" class="reconnect-banner" role="status">{{ t("page.reconnecting") }}</p>
        <RouterView :key="settingsRoute ? 'settings' : route.fullPath" />
      </main>

      <section v-if="splitId" class="split-region" :aria-label="t('split.region')" :data-active="ui.activePane === 'split'" @pointerdown="ui.activePane = 'split'" @focusin="ui.activePane = 'split'">
        <ChatPage :key="splitId" :session-id="splitId" pane="split" />
      </section>

      <div
        v-if="detailOpen"
        class="detail-resizer"
        role="separator"
        aria-orientation="vertical"
        :aria-label="t('lines.resize')"
        :aria-valuenow="ui.detailWidth"
        :aria-valuemin="320"
        :aria-valuemax="720"
        tabindex="0"
        @pointerdown.prevent="startResize"
        @keydown.left.prevent="resizeBy(-16)"
        @keydown.right.prevent="resizeBy(16)"
      />
      <aside v-if="detailOpen && sessionId" class="detail-region" data-active="true" :aria-label="t('detail.panel')" @pointerdown="ui.detailFocused = true" @focusin="ui.detailFocused = true">
        <DetailPanel :session-id="sessionId" />
      </aside>
    </div>

    <NewLineDialog v-if="ui.newLine" />
    <EndLineDialog v-if="ui.endingLine" />
    <ShortcutsDialog />
  </div>
</template>
