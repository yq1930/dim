<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue"
import { RouterView, useRoute } from "vue-router"
import { useI18n } from "vue-i18n"
import { Menu, PanelLeftOpen, X } from "lucide-vue-next"
import AppSidebar from "../components/AppSidebar.vue"
import DetailPanel from "../components/DetailPanel.vue"
import ToastRegion from "../components/ToastRegion.vue"
import ChatPage from "../features/chat/ChatPage.vue"
import EndLineDialog from "../features/lines/EndLineDialog.vue"
import NewLineDialog from "../features/lines/NewLineDialog.vue"
import { settingsMessages } from "../i18n/settings"
import { useConnectionStore } from "../stores/connection"
import { useSessionsStore, workspaceName } from "../stores/sessions"
import { useTerminalsStore } from "../stores/terminals"
import { useUiStore } from "../stores/ui"

const route = useRoute()
const { t } = useI18n()
const { t: settingsText } = useI18n({ messages: settingsMessages })
const connection = useConnectionStore()
const sessions = useSessionsStore()
const ui = useUiStore()
const terminals = useTerminalsStore()
const root = ref<HTMLElement>()
const drawer = ref<HTMLDialogElement>()
let observer: ResizeObserver | undefined

const settingsRoute = computed(() => route.path === "/settings" || route.path.startsWith("/settings/"))
const chatRoute = computed(() => route.name === "chat-new" || route.name === "chat-session")
const sessionId = computed(() => typeof route.params.sessionId === "string" ? route.params.sessionId : undefined)
const detailOpen = computed(() => ui.detailOpen && route.name === "chat-session" && sessionId.value !== undefined)
const splitId = computed(() => {
  const id = ui.splitSessionId
  return route.name === "chat-session" && id !== undefined && id !== sessionId.value && ui.splitAvailable && sessions.byId.has(id) ? id : undefined
})
watch(splitId, (id) => { if (id === undefined) ui.activePane = "main" })
const pageTitle = computed(() => {
  if (settingsRoute.value) return settingsText("settings")
  if (route.name === "chat-session" && typeof route.params.sessionId === "string") {
    const session = sessions.byId.get(route.params.sessionId)
    return session?.title ?? session?.preview ?? (session === undefined ? t("route.session") : workspaceName(session.cwd))
  }
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

function onKey(event: KeyboardEvent): void {
  if (event.key === "Escape") {
    for (const menu of document.querySelectorAll<HTMLDetailsElement>(POPOVERS)) menu.open = false
    return
  }
  // Ctrl+` shows or hides the Session's terminals, as in editors.
  if (event.ctrlKey && !event.metaKey && !event.altKey && event.code === "Backquote" && route.name === "chat-session" && terminals.available) {
    event.preventDefault()
    ui.toggleDetail("terminal")
    return
  }
  // ⌘B / Ctrl+B toggles the sidebar: the drawer in compact mode, the inline rail otherwise.
  const mod = navigator.userAgent.includes("Mac") ? event.metaKey : event.ctrlKey
  if (mod && !event.altKey && !event.shiftKey && event.key.toLowerCase() === "b") {
    event.preventDefault()
    if (ui.shellMode === "compact") ui.drawerOpen = !ui.drawerOpen
    else ui.sidebarCollapsed = !ui.sidebarCollapsed
  }
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

    <header v-if="ui.shellMode === 'compact' && !settingsRoute" class="compact-header">
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
        <button v-if="ui.sidebarCollapsed && ui.shellMode !== 'compact' && !settingsRoute" class="ui-icon-button sidebar-reopen" data-variant="ghost" data-size="md" type="button" :aria-label="t('sidebar.openMenu')" v-tooltip="t('sidebar.openMenu')" @click="ui.sidebarCollapsed = false">
          <PanelLeftOpen aria-hidden="true" />
        </button>
        <header v-if="!settingsRoute" class="main-header">
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
  </div>
</template>
