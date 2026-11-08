<script setup lang="ts">
import { Monitor, Moon, Sun } from "lucide-vue-next"
import { useI18n } from "vue-i18n"
import { useSettingsText } from "../../i18n/settings-general"
import { useAppearanceStore } from "../../stores/appearance"
import { TRANSCRIPT_VIEWS, usePreferencesStore } from "../../stores/preferences"

const s = useSettingsText()
const { t } = useI18n()
const appearance = useAppearanceStore()
const preferences = usePreferencesStore()
const themes = [{ value: 'light', icon: Sun }, { value: 'dark', icon: Moon }, { value: 'system', icon: Monitor }] as const
function selectLanguage(event: Event): void {
  const value = (event.target as HTMLSelectElement).value
  if (value === "" || value === "en" || value === "zh-CN") void appearance.selectLanguage(value)
}
</script>

<template>
  <article class="settings-page">
    <header><h2 data-route-heading tabindex="-1">{{ s('Appearance', '外观') }}</h2><p>{{ s('Changes apply immediately and are saved in this browser, independently of the CLI.', '更改立即生效并保存在此浏览器中，与 CLI 设置相互独立。') }}</p></header>
    <p v-if="appearance.error" class="ui-banner settings-banner" data-tone="danger" role="alert">{{ appearance.error }}</p>
    <section class="settings-card"><h3>{{ s('Theme', '主题') }}</h3><div class="theme-options" role="group" :aria-label="s('Theme', '主题')"><button v-for="theme in themes" :key="theme.value" class="theme-option" :class="{ 'is-selected': appearance.theme === theme.value }" type="button" :aria-pressed="appearance.theme === theme.value" @click="appearance.theme = theme.value"><component :is="theme.icon" :size="22" aria-hidden="true" /><span>{{ theme.value === 'light' ? s('Light', '浅色') : theme.value === 'dark' ? s('Dark', '深色') : s('System', '跟随系统') }}</span></button></div></section>
    <section class="settings-card"><h3>{{ s('UI font size', '界面字号') }}</h3><div class="settings-actions" role="group" :aria-label="s('UI font size', '界面字号')"><button v-for="size in [12, 13, 14, 15, 16, 18]" :key="size" class="ui-button" data-size="md" :data-variant="appearance.fontSize === size ? 'primary' : 'secondary'" :aria-pressed="appearance.fontSize === size" @click="appearance.fontSize = size">{{ size }}</button></div><p>{{ s('Scales the entire interface, including controls and chat text.', '缩放整个界面，包括控件和聊天文本。') }}</p></section>
    <section class="settings-card"><h3>{{ s('Default transcript view', '默认对话视图') }}</h3><div class="settings-actions" role="group" :aria-label="s('Default transcript view', '默认对话视图')"><button v-for="view in TRANSCRIPT_VIEWS" :key="view" class="ui-button" data-size="md" :data-variant="preferences.transcriptView === view ? 'primary' : 'secondary'" :aria-pressed="preferences.transcriptView === view" @click="preferences.transcriptView = view">{{ t(`transcript.${view}`) }}</button></div><p>{{ s('Normal folds the agent\'s work into summaries, Thinking shows its thinking, Verbose opens every step. Picking a view from a session\'s menu, or pressing Ctrl+O, changes only that session.', '标准把 Agent 的工作折叠成摘要，思考展开思考过程，详细展开每一步。在会话的“⋯”菜单中切换或按 Ctrl+O，只影响该会话。') }}</p></section>
    <section class="settings-card"><h3>{{ s('Language', '语言') }}</h3><label><span>{{ s('Interface language', '界面语言') }}</span><select class="ui-field" data-size="md" :value="appearance.language" @change="selectLanguage"><option value="">{{ s('System default', '跟随系统') }}</option><option value="en">English</option><option value="zh-CN">简体中文</option></select></label></section>
  </article>
</template>

<style scoped>
.theme-options { display: grid; grid-template-columns: repeat(3, 1fr); gap: var(--space-3); }
.theme-option { display: flex; min-width: 0; flex-direction: column; align-items: center; gap: var(--space-3); padding: var(--space-5) var(--space-2); border: 1px solid var(--border); border-radius: var(--radius-lg); background: transparent; color: var(--foreground); font-size: var(--text-md); cursor: pointer; }
.theme-option:hover { background: var(--hover); }
.theme-option.is-selected { border-color: var(--primary); background: var(--primary-soft); color: var(--primary); }
.theme-option:focus-visible { outline: 2px solid var(--primary); outline-offset: 3px; }
label { display: grid; gap: var(--space-2); }
</style>
