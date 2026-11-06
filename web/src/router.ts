import { createRouter, createWebHistory } from "vue-router"
import ChatPage from "./features/chat/ChatPage.vue"
import NewSessionPage from "./features/chat/NewSessionPage.vue"
import RootRedirect from "./views/RootRedirect.vue"
import NotFoundView from "./views/NotFoundView.vue"
import { useUiStore } from "./stores/ui"

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: "/", name: "root", component: RootRedirect },
    { path: "/chat/new", name: "chat-new", component: NewSessionPage },
    { path: "/chat/:sessionId", name: "chat-session", component: ChatPage, props: true },
    {
      path: "/settings", component: () => import("./features/settings/SettingsPage.vue"), children: [
        { path: "", name: "settings-index", component: { render: () => null } },
        { path: "account", name: "settings-account", component: () => import("./features/settings/AccountSettings.vue") },
        { path: "appearance", name: "settings-appearance", component: () => import("./features/settings/AppearanceSettings.vue") },
        { path: "agent", name: "settings-agent", component: () => import("./features/settings/AgentSettings.vue") },
        { path: "providers", name: "settings-providers", component: () => import("./features/settings/ProviderSettings.vue") },
        { path: "providers/new", name: "settings-provider-new", component: () => import("./features/settings/ProviderSettings.vue") },
        { path: "providers/:providerId", name: "settings-provider-detail", component: () => import("./features/settings/ProviderSettings.vue") },
        { path: "modality", name: "settings-modality", component: () => import("./features/settings/ModalitySettings.vue") },
        { path: "mcp/:serverId?", name: "settings-mcp", component: () => import("./features/settings/McpSettings.vue") },
        { path: "plugins/:pluginId?", name: "settings-plugins", component: () => import("./features/settings/PluginSettings.vue") },
        { path: "skills/:skillId?", name: "settings-skills", component: () => import("./features/settings/SkillSettings.vue") },
        { path: "web", name: "settings-web", component: () => import("./features/settings/WebSearchSettings.vue") },
        { path: "permissions/:page?", name: "settings-permissions", component: () => import("./features/settings/PermissionsSettings.vue") },
        { path: "archived", name: "settings-archived", component: () => import("./features/settings/ArchivedSettings.vue") },
        { path: "diagnostics", name: "settings-diagnostics", component: () => import("./features/settings/DiagnosticsSettings.vue") },
      ],
    },
    { path: "/:pathMatch(.*)*", name: "not-found", component: NotFoundView },
  ],
})

router.afterEach((to) => {
  const ui = useUiStore()
  const sessionId = to.name === "chat-session" && typeof to.params.sessionId === "string" ? to.params.sessionId : undefined
  ui.viewSession(sessionId)
  if (sessionId !== undefined) ui.lastSessionId = sessionId
})
