import "@fontsource-variable/geist"
import "@fontsource-variable/geist-mono"
import "markstream-vue/index.css"
import "./styles/global.css"
import "./styles/controls.css"
import "./styles/settings.css"
import { createApp, watch } from "vue"
import { createPinia } from "pinia"
import AppRoot from "./app/AppRoot.vue"
import { vTooltip } from "./components/tooltip"
import { loadMarkdownRenderer } from "./features/chat/markdown-renderer"
import { i18n, setAppLocale } from "./i18n"
import { router } from "./router"
import { exchangeBootstrapToken } from "./rpc/client"
import { overlayTitleBar } from "./desktop"
import { useAppearanceStore } from "./stores/appearance"
import { useConnectionStore } from "./stores/connection"
import { useUiStore } from "./stores/ui"

watch(i18n.global.locale, (locale) => { document.documentElement.lang = locale }, { immediate: true })
if (overlayTitleBar) document.documentElement.dataset.titlebar = "overlay"
const pinia = createPinia()
useUiStore(pinia)

void Promise.all([exchangeBootstrapToken(), setAppLocale(useAppearanceStore(pinia).language || navigator.language)]).then(() => {
  useConnectionStore(pinia).client.start()
  createApp(AppRoot).use(pinia).use(i18n).use(router).directive("tooltip", vTooltip).mount("#app")
  setTimeout(() => void loadMarkdownRenderer(), 0)
})
