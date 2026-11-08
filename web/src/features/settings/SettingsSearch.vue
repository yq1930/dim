<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, useId, watch } from "vue"
import { useRouter } from "vue-router"
import { useI18n } from "vue-i18n"
import { Search } from "lucide-vue-next"
import { settingsMessages } from "../../i18n/settings"
import { useSettingsText } from "../../i18n/settings-general"
import { shortcutContext } from "../../app/shortcuts"
import { searchSettings, type SettingsHit } from "./registry"

/** The settings search: pages and preference rows, opened at the row. */
const query = defineModel<string>({ required: true })
const { t } = useI18n({ messages: settingsMessages })
const s = useSettingsText()
const router = useRouter()
const id = useId()
const field = ref<HTMLInputElement>()
const active = ref(0)

const hits = computed(() => searchSettings(query.value, (page) => `${t(page)} ${settingsMessages.en[page]} ${settingsMessages["zh-CN"][page]}`))
watch(query, () => { active.value = 0 })

function label(hit: SettingsHit): string {
  return hit.setting === undefined ? t(hit.page) : s(hit.setting.label[0], hit.setting.label[1])
}

async function open(hit: SettingsHit): Promise<void> {
  query.value = ""
  await router.push({ path: `/settings/${hit.page}`, hash: hit.setting === undefined ? "" : `#setting-${hit.setting.id}` })
}

function onKey(event: KeyboardEvent): void {
  if (event.isComposing) return
  const count = hits.value.length
  if (event.key === "ArrowDown" || event.key === "ArrowUp") {
    event.preventDefault()
    if (count > 0) active.value = (active.value + (event.key === "ArrowDown" ? 1 : -1) + count) % count
  } else if (event.key === "Enter") {
    event.preventDefault()
    const hit = hits.value[active.value]
    if (hit !== undefined) void open(hit)
  } else if (event.key === "Escape" && query.value !== "") {
    event.preventDefault()
    event.stopPropagation()
    query.value = ""
  }
}

/** ⌘F (Ctrl+F) finds a setting, as in Claude Code's settings. */
function onFind(event: KeyboardEvent): void {
  const mac = shortcutContext().mac
  if ((mac ? !event.metaKey || event.ctrlKey : !event.ctrlKey || event.metaKey) || event.altKey || event.shiftKey || event.key.toLowerCase() !== "f") return
  if (document.querySelector("dialog[open]") !== null) return
  event.preventDefault()
  field.value?.focus()
  field.value?.select()
}
onMounted(() => document.addEventListener("keydown", onFind))
onBeforeUnmount(() => document.removeEventListener("keydown", onFind))
</script>

<template>
  <div class="settings-search">
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
        :aria-expanded="query !== ''"
        :aria-controls="`${id}-results`"
        :aria-activedescendant="query !== '' && hits.length > 0 ? `${id}-${active}` : undefined"
        :placeholder="t('findSetting')"
        :aria-label="t('findSetting')"
        @keydown="onKey"
      >
    </div>
    <ul v-if="query !== ''" :id="`${id}-results`" class="settings-results" role="listbox" :aria-label="t('findSetting')">
      <li
        v-for="(hit, index) in hits"
        :id="`${id}-${index}`"
        :key="`${hit.page}:${hit.setting?.id ?? ''}`"
        role="option"
        :aria-selected="index === active"
        @mousemove="active = index"
        @click="open(hit)"
      >
        <span>{{ label(hit) }}</span>
        <small v-if="hit.setting">{{ t(hit.page) }}</small>
      </li>
      <li v-if="hits.length === 0" class="settings-results-empty" role="status">{{ t("noSetting") }}</li>
    </ul>
  </div>
</template>

<style scoped>
.settings-search { margin-bottom: var(--space-3); }
.settings-results { display: grid; gap: 2px; margin: var(--space-2) 0 0; padding: 0; list-style: none; }
.settings-results [role="option"] { display: flex; min-width: 0; flex-direction: column; gap: 2px; padding: 8px 12px; border-radius: var(--radius-lg); color: var(--fg-2); font-size: var(--text-md); cursor: pointer; }
.settings-results [role="option"] > span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.settings-results [role="option"] > small { color: var(--muted-foreground); font-size: var(--text-xs); }
.settings-results [aria-selected="true"] { background: var(--accent); color: var(--foreground); }
.settings-results-empty { padding: var(--space-4) 12px; color: var(--muted-foreground); font-size: var(--text-md); }
</style>
