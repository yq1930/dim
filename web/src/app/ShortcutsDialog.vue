<script setup lang="ts">
import { computed, nextTick, ref, useId, watch } from "vue"
import { useI18n } from "vue-i18n"
import { Search, X } from "lucide-vue-next"
import { useUiStore } from "../stores/ui"
import { SHORTCUT_GROUPS, SHORTCUT_HELP, keyCaps, shortcutContext, shortcutKeys } from "./shortcuts"

/** Every keyboard shortcut of the app by group, filtered by what is typed. */
const { t } = useI18n()
const ui = useUiStore()
const id = useId()
const dialog = ref<HTMLDialogElement>()
const field = ref<HTMLInputElement>()
const query = ref("")
const context = shortcutContext()

/** Each row with its alternative keys as key caps; a command without keys here is left out. */
const rows = computed(() => SHORTCUT_HELP.flatMap((row) => {
  const keys = "command" in row ? shortcutKeys(row.command, context) : row.keys
  if (keys.length === 0) return []
  const alternatives = keys.map((key) => keyCaps(key, context.mac))
  const label = t(row.label)
  return [{ group: row.group, label, alternatives, search: `${label} ${alternatives.flat().join(" ")}`.toLocaleLowerCase() }]
}))
const groups = computed(() => {
  const words = query.value.trim().toLocaleLowerCase().split(/\s+/).filter((word) => word !== "")
  const shown = rows.value.filter((row) => words.every((word) => row.search.includes(word)))
  return SHORTCUT_GROUPS.map((group) => ({ group, rows: shown.filter((row) => row.group === group) })).filter((entry) => entry.rows.length > 0)
})

watch(() => ui.shortcutsOpen, async (open) => {
  await nextTick()
  const element = dialog.value
  if (element === undefined) return
  if (open && !element.open) {
    query.value = ""
    element.showModal()
    field.value?.focus()
  } else if (!open && element.open) element.close()
}, { immediate: true })

/** A press on the backdrop, outside the dialog's box, closes it. */
function onClick(event: MouseEvent): void {
  const element = dialog.value
  if (element === undefined || event.target !== element) return
  const box = element.getBoundingClientRect()
  if (event.clientX < box.left || event.clientX > box.right || event.clientY < box.top || event.clientY > box.bottom) element.close()
}
</script>

<template>
  <dialog ref="dialog" class="ui-dialog shortcuts-dialog" data-size="md" :aria-labelledby="id" @close="ui.shortcutsOpen = false" @click="onClick">
    <header class="ui-dialog-header">
      <h2 :id="id" class="ui-dialog-title">{{ t("shortcuts.title") }}</h2>
      <button type="button" class="ui-icon-button ui-dialog-close" data-variant="ghost" data-size="sm" :aria-label="t('common.close')" @click="dialog?.close()"><X aria-hidden="true" /></button>
    </header>
    <div class="ui-field-wrap shortcuts-search">
      <Search aria-hidden="true" />
      <input ref="field" v-model="query" class="ui-field" data-size="md" type="search" :placeholder="t('shortcuts.search')" :aria-label="t('shortcuts.search')">
    </div>
    <div class="shortcuts-body">
      <section v-for="entry in groups" :key="entry.group" class="shortcuts-group" :aria-labelledby="`${id}-${entry.group}`">
        <h3 :id="`${id}-${entry.group}`">{{ t(`shortcuts.group.${entry.group}`) }}</h3>
        <dl>
          <div v-for="row in entry.rows" :key="row.label" class="shortcuts-row">
            <dt>{{ row.label }}</dt>
            <dd>
              <template v-for="(caps, index) in row.alternatives" :key="index">
                <span v-if="index > 0" class="shortcuts-or">/</span>
                <kbd v-for="cap in caps" :key="cap" class="ui-kbd">{{ cap }}</kbd>
              </template>
            </dd>
          </div>
        </dl>
      </section>
      <p v-if="groups.length === 0" class="shortcuts-empty" role="status">{{ t("shortcuts.none") }}</p>
    </div>
  </dialog>
</template>

<style scoped>
/* A fixed height keeps the dialog in place while the list filters. */
.shortcuts-dialog[open] { display: flex; height: min(640px, calc(100dvh - var(--space-8))); flex-direction: column; }
.shortcuts-search { margin-top: var(--space-4); }
.shortcuts-body { min-height: 0; flex: 1; overflow-y: auto; margin: var(--space-3) calc(-1 * var(--space-5)) calc(-1 * var(--space-5)); padding: 0 var(--space-5) var(--space-5); }
.shortcuts-group + .shortcuts-group { margin-top: var(--space-4); }
.shortcuts-group h3 { margin: 0 0 var(--space-1); color: var(--muted-foreground); font-size: var(--text-xs); font-weight: var(--weight-medium); }
.shortcuts-group dl { margin: 0; }
.shortcuts-row { display: flex; min-height: 32px; align-items: center; justify-content: space-between; gap: var(--space-4); border-bottom: 1px solid var(--border); }
.shortcuts-row:last-child { border-bottom: 0; }
.shortcuts-row dt { min-width: 0; color: var(--foreground); font-size: var(--text-md); }
.shortcuts-row dd { display: flex; flex: none; align-items: center; gap: var(--space-1); margin: 0; }
.shortcuts-row .ui-kbd { min-width: 22px; height: 22px; padding: 0 var(--space-1); font-family: var(--font-sans); font-size: var(--text-sm); }
.shortcuts-or { color: var(--muted-foreground); font-size: var(--text-xs); }
.shortcuts-empty { margin: 0; padding: var(--space-6) 0 var(--space-2); color: var(--muted-foreground); text-align: center; }
</style>
