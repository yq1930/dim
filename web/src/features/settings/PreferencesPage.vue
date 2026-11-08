<script setup lang="ts">
import { computed, nextTick, watch } from "vue"
import { useRoute } from "vue-router"
import { useI18n } from "vue-i18n"
import { settingsMessages } from "../../i18n/settings"
import { useSettingsText } from "../../i18n/settings-general"
import { useAppearanceStore } from "../../stores/appearance"
import { SETTINGS, type Bilingual, type SettingOption } from "./registry"

/** A page of the browser's preferences, its rows drawn from the settings registry. */
const props = defineProps<{ page: "appearance" | "general" }>()
const s = useSettingsText()
const { t } = useI18n()
const { t: pageText } = useI18n({ messages: settingsMessages })
const appearance = useAppearanceStore()
const route = useRoute()
const rows = computed(() => SETTINGS.filter((setting) => setting.page === props.page && (setting.available?.() ?? true)))

function text(value: Bilingual): string {
  return s(value[0], value[1])
}

function optionLabel(option: SettingOption): string {
  return typeof option.label === "string" ? t(option.label) : text(option.label)
}

/** A row the settings search opened scrolls into view and flashes. */
watch(() => route.hash, async (hash) => {
  if (!hash.startsWith("#setting-")) return
  await nextTick()
  const row = document.getElementById(hash.slice(1))
  if (row === null) return
  row.scrollIntoView({ block: "center" })
  row.removeAttribute("data-highlighted")
  void row.offsetWidth
  row.setAttribute("data-highlighted", "")
}, { immediate: true })
</script>

<template>
  <article class="settings-page">
    <header>
      <h2 data-route-heading tabindex="-1">{{ pageText(page) }}</h2>
      <p>{{ s('Changes apply immediately and are saved in this browser.', '更改立即生效，保存在此浏览器中。') }}</p>
    </header>
    <p v-if="appearance.error" class="ui-banner settings-banner" data-tone="danger" role="alert">{{ appearance.error }}</p>
    <section v-for="row in rows" :id="`setting-${row.id}`" :key="row.id" class="settings-card setting-row" :data-control="row.control" @animationend="($event.currentTarget as HTMLElement).removeAttribute('data-highlighted')">
      <div class="setting-text">
        <h3 :id="`setting-${row.id}-label`">{{ text(row.label) }}</h3>
        <p v-if="row.description">{{ text(row.description) }}</p>
      </div>
      <button v-if="row.control === 'switch'" type="button" role="switch" class="ui-switch" :aria-checked="row.get()" :aria-labelledby="`setting-${row.id}-label`" @click="row.set(!row.get())" />
      <select v-else-if="row.control === 'select'" class="ui-field" data-size="md" :value="row.get()" :aria-labelledby="`setting-${row.id}-label`" @change="row.set(($event.target as HTMLSelectElement).value)">
        <option v-for="option in row.options" :key="option.value" :value="option.value">{{ optionLabel(option) }}</option>
      </select>
      <div v-else :class="row.control === 'cards' ? 'setting-cards' : 'settings-actions'" role="group" :aria-labelledby="`setting-${row.id}-label`">
        <button
          v-for="option in row.options"
          :key="option.value"
          type="button"
          :class="row.control === 'cards' ? 'setting-card-option' : 'ui-button'"
          data-size="md"
          :data-variant="row.control === 'cards' ? undefined : row.get() === option.value ? 'primary' : 'secondary'"
          :aria-pressed="row.get() === option.value"
          @click="row.set(option.value)"
        >
          <component :is="option.icon" v-if="option.icon" :size="22" aria-hidden="true" />
          <span>{{ optionLabel(option) }}</span>
        </button>
      </div>
    </section>
  </article>
</template>

<style scoped>
.setting-row { display: grid; gap: var(--space-4); scroll-margin: var(--space-6); }
.setting-row[data-control="switch"] { grid-template-columns: minmax(0, 1fr) auto; align-items: start; }
.setting-row[data-control="switch"] > .ui-switch { margin-top: 2px; }
.setting-row[data-control="select"] > .ui-field { max-width: 320px; }
.setting-text h3 { margin: 0; }
.setting-text p { margin: var(--space-2) 0 0; color: var(--muted-foreground); font-size: var(--text-sm); line-height: var(--leading-normal); }
.setting-row[data-highlighted] { animation: setting-flash 1.6s var(--ease-out); }
@keyframes setting-flash {
  0%, 40% { border-color: var(--primary); box-shadow: 0 0 0 3px var(--primary-soft); }
  100% { border-color: var(--border); box-shadow: none; }
}
.setting-cards { display: grid; grid-template-columns: repeat(3, 1fr); gap: var(--space-3); }
.setting-card-option { display: flex; min-width: 0; flex-direction: column; align-items: center; gap: var(--space-3); padding: var(--space-5) var(--space-2); border: 1px solid var(--border); border-radius: var(--radius-lg); background: transparent; color: var(--foreground); font-size: var(--text-md); cursor: pointer; }
.setting-card-option:hover { background: var(--hover); }
.setting-card-option[aria-pressed="true"] { border-color: var(--primary); background: var(--primary-soft); color: var(--primary); }
.setting-card-option:focus-visible { outline: 2px solid var(--primary); outline-offset: 3px; }
</style>
