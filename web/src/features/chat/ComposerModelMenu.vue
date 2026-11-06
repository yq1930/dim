<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from "vue"
import { useI18n } from "vue-i18n"
import { RouterLink } from "vue-router"
import { Check, ChevronRight, LoaderCircle, Search, Settings, Star } from "lucide-vue-next"
import type { ModelId } from "../../rpc/protocol"
import { useModelsStore } from "../../stores/models"
import { useConnectionStore } from "../../stores/connection"
import { useToastStore } from "../../stores/toast"

/**
 * The model picker: the current provider's models, numbered for the keyboard,
 * then the reasoning effort and the other providers' models in submenus, then
 * the default-model and settings actions. When the model cannot be changed (no
 * provider methods, or no `session.model.update` on this connection) it only
 * names the current model.
 */
type Option = { providerId: string; providerName: string; modelId: string; name: string; efforts: string[] }
type Submenu = "effort" | "more"

/** How many of the current provider's models the menu lists with number keys. */
const PRIMARY = 9
const SUBMENU_WIDTH = 280
const EFFORT_LABELS: Record<string, string> = {
  none: "composer.effortNone", minimal: "composer.effortMinimal", low: "composer.effortLow",
  medium: "composer.effortMedium", high: "composer.effortHigh", xhigh: "composer.effortXhigh",
  max: "composer.effortMax", ultra: "composer.effortUltra",
}

const props = defineProps<{
  model: ModelId | undefined
  selectable: boolean
  disabled: boolean
  /** The reasoning effort in use; undefined is the provider's default. */
  effort?: string
  /** False where the effort cannot be changed here. */
  effortSelectable: boolean
}>()
const emit = defineEmits<{ select: [model: ModelId]; effort: [effort: string | undefined] }>()
const { t } = useI18n()
const models = useModelsStore()
const connection = useConnectionStore()
const savingDefault = ref(false)
const details = ref<HTMLDetailsElement>()
const content = ref<HTMLElement>()
const searchInput = ref<HTMLInputElement>()
const query = ref("")
const submenu = ref<Submenu>()
/** Where the open submenu sits: beside the menu, or in it on a narrow window. */
const placement = ref<{ side: "left" | "right" | "inline"; bottom: number }>({ side: "right", bottom: 0 })

const providers = computed(() => models.providers ?? [])
const options = computed<Option[]>(() => providers.value.flatMap((provider) => provider.models.map((model) => ({
  providerId: provider.providerId,
  providerName: provider.name,
  modelId: model.id,
  name: model.name,
  efforts: model.reasoningEfforts ?? [],
}))))
const ownProvider = computed(() => providers.value.find((provider) => provider.providerId === props.model?.providerId) ?? providers.value[0])
/** The current provider's first models, the current one always among them. */
const primary = computed(() => {
  const own = options.value.filter((option) => option.providerId === ownProvider.value?.providerId)
  const listed = own.slice(0, PRIMARY)
  const index = own.findIndex(isCurrent)
  if (index >= PRIMARY) listed[PRIMARY - 1] = own[index]!
  return listed
})
const others = computed(() => options.value.filter((option) => !primary.value.includes(option)))
/** "More models": the others by provider, or every match while searching. */
const groups = computed(() => {
  const needle = query.value.trim().toLocaleLowerCase()
  const listed = needle === ""
    ? others.value
    : options.value.filter((option) => [option.name, option.modelId, option.providerName].some((value) => value.toLocaleLowerCase().includes(needle)))
  const byProvider = new Map<string, { providerId: string; name: string; options: Option[] }>()
  for (const option of listed) {
    const group = byProvider.get(option.providerId) ?? { providerId: option.providerId, name: option.providerName, options: [] }
    group.options.push(option)
    byProvider.set(option.providerId, group)
  }
  return [...byProvider.values()]
})
const efforts = computed(() => props.effortSelectable ? options.value.find(isCurrent)?.efforts ?? [] : [])
const triggerLabel = computed(() => props.model === undefined ? t("composer.notConfigured") : models.nameOf(props.model))
/** The provider's name for its badge; a provider id is often a UUID. */
const providerName = computed(() => providers.value.find((provider) => provider.providerId === props.model?.providerId)?.name ?? props.model?.providerId ?? "")
const effortLabel = computed(() => props.effort === undefined ? undefined : label(props.effort))

// The trigger names the model from the catalog once it is loaded.
onMounted(() => {
  if (props.selectable && models.available && models.providers === undefined) void models.load()
})

function isCurrent(option: Option): boolean {
  return option.providerId === props.model?.providerId && option.modelId === props.model.modelId
}

function label(effort: string): string {
  return EFFORT_LABELS[effort] ? t(EFFORT_LABELS[effort]!) : effort
}

function initial(name: string): string {
  return name.trim().charAt(0).toLocaleUpperCase()
}

function close(): void {
  if (details.value) details.value.open = false
}

async function onToggle(): Promise<void> {
  submenu.value = undefined
  query.value = ""
  if (details.value?.open !== true) return
  if (models.providers === undefined) await models.load()
  await nextTick()
  if (matchMedia("(pointer: coarse)").matches) return
  content.value?.querySelector<HTMLElement>('[role="menuitemradio"][aria-checked="true"], [role="menuitemradio"]')?.focus()
}

/** Opens a submenu beside the menu, its bottom level with its row. */
async function openSubmenu(kind: Submenu, row: EventTarget | null): Promise<void> {
  const box = content.value?.getBoundingClientRect()
  const rect = row instanceof HTMLElement ? row.getBoundingClientRect() : undefined
  if (box !== undefined && rect !== undefined) {
    const side = window.innerWidth < 720 ? "inline"
      : box.right + SUBMENU_WIDTH + 8 <= window.innerWidth ? "right" : "left"
    placement.value = { side, bottom: box.bottom - rect.bottom }
  }
  if (submenu.value === kind) return
  submenu.value = kind
  query.value = ""
  if (kind === "more") {
    await nextTick()
    if (!matchMedia("(pointer: coarse)").matches) searchInput.value?.focus()
  }
}

function toggleSubmenu(kind: Submenu, event: Event): void {
  if (submenu.value === kind) submenu.value = undefined
  else void openSubmenu(kind, event.currentTarget)
}

function select(option: Option): void {
  close()
  if (!isCurrent(option)) emit("select", { providerId: option.providerId, modelId: option.modelId })
}

function chooseEffort(effort: string | undefined): void {
  close()
  if (effort !== props.effort) emit("effort", effort)
}

async function makeDefault(): Promise<void> {
  const model = props.model
  if (!model || savingDefault.value) return
  savingDefault.value = true
  try {
    await models.setGlobalDefault(model)
    close()
    useToastStore().push({ tone: "success", title: t("composer.defaultSaved", { model: models.nameOf(model) }) })
  } catch (cause) { useToastStore().push({ tone: "danger", title: t("composer.defaultFailed"), description: cause instanceof Error ? cause.message : String(cause) }) }
  finally { savingDefault.value = false }
}

/** Number keys pick a listed model; Escape closes a submenu, then the menu. */
function onKey(event: KeyboardEvent): void {
  if (event.isComposing || event.keyCode === 229 || event.metaKey || event.ctrlKey || event.altKey) return
  if (event.key === "Escape") {
    event.preventDefault()
    if (submenu.value !== undefined) {
      submenu.value = undefined
      return
    }
    close()
    details.value?.querySelector("summary")?.focus()
    return
  }
  if (event.target instanceof HTMLInputElement) return
  const number = Number(event.key)
  if (Number.isInteger(number) && number >= 1 && number <= primary.value.length) {
    event.preventDefault()
    select(primary.value[number - 1]!)
  }
}

defineExpose({ open: () => {
  if (props.selectable && models.available && !props.disabled && details.value) details.value.open = true
} })
</script>

<template>
  <span v-if="!selectable || !models.available" class="ui-button model-trigger" data-variant="ghost" data-size="sm" aria-disabled="true">
    <span class="model-trigger-label">{{ triggerLabel }}</span>
  </span>
  <details v-else ref="details" class="composer-popover model-menu" :class="{ 'is-disabled': disabled }" @toggle="onToggle">
    <summary class="ui-button model-trigger" data-variant="ghost" data-size="sm" v-tooltip="model ? `${triggerLabel} · ${providerName}` : undefined">
      <span v-if="model" class="provider-badge" aria-hidden="true">{{ initial(providerName) }}</span>
      <span class="model-trigger-label">{{ triggerLabel }}</span>
      <span v-if="effortLabel" class="model-trigger-effort">{{ effortLabel }}</span>
    </summary>
    <div ref="content" class="composer-popover-content is-end ui-popover model-content" data-width="lg" role="menu" :aria-label="t('composer.command.models')" @keydown="onKey">
      <p v-if="models.providers === undefined && !models.error" class="menu-empty"><LoaderCircle class="is-spinning" aria-hidden="true" />{{ t("composer.modelLoading") }}</p>
      <p v-else-if="models.error && providers.length === 0" class="menu-empty">
        <button type="button" class="ui-button" data-variant="danger-ghost" data-size="sm" @click="models.load()">{{ t("composer.modelLoadRetry") }}</button>
      </p>
      <template v-else>
        <p v-if="providers.length > 1 && ownProvider" class="ui-menu-label">{{ ownProvider.name }}</p>
        <div class="model-list">
          <p v-if="primary.length === 0" class="menu-empty">{{ t("composer.noModels") }}</p>
          <button
            v-for="(option, index) in primary"
            :key="`${option.providerId}:${option.modelId}`"
            type="button"
            role="menuitemradio"
            class="ui-menu-item"
            :aria-checked="isCurrent(option)"
            :title="`${option.name} · ${option.providerName}`"
            @mouseenter="submenu = undefined"
            @click="select(option)"
          >
            <span class="model-name">{{ option.name }}</span>
            <Check v-if="isCurrent(option)" class="ui-menu-check" aria-hidden="true" />
            <kbd v-else class="ui-menu-meta">{{ index + 1 }}</kbd>
          </button>
        </div>
        <hr v-if="efforts.length || others.length" class="ui-menu-separator">
        <div v-if="efforts.length" class="model-submenu">
          <button
            type="button"
            class="ui-menu-item model-submenu-row"
            aria-haspopup="menu"
            :aria-expanded="submenu === 'effort'"
            :data-highlighted="submenu === 'effort' ? '' : undefined"
            @mouseenter="openSubmenu('effort', $event.currentTarget)"
            @click="toggleSubmenu('effort', $event)"
            @keydown.right.prevent="openSubmenu('effort', $event.currentTarget)"
          >
            <span>{{ t("composer.reasoningEffort") }}</span>
            <span class="model-submenu-value">{{ effortLabel ?? t("composer.effortDefault") }}</span>
            <ChevronRight aria-hidden="true" />
          </button>
          <div v-if="submenu === 'effort'" class="ui-popover model-submenu-panel" role="menu" :aria-label="t('composer.reasoningEffort')" :data-side="placement.side" :style="placement.side === 'inline' ? undefined : { bottom: `${placement.bottom}px` }">
            <p class="model-submenu-hint">{{ t("composer.effortHint") }}</p>
            <button type="button" role="menuitemradio" class="ui-menu-item" :aria-checked="effort === undefined" @click="chooseEffort(undefined)">
              <span>{{ t("composer.effortDefault") }}</span>
              <Check v-if="effort === undefined" class="ui-menu-check" aria-hidden="true" />
            </button>
            <button v-for="choice in efforts" :key="choice" type="button" role="menuitemradio" class="ui-menu-item" :aria-checked="effort === choice" @click="chooseEffort(choice)">
              <span>{{ label(choice) }}</span>
              <Check v-if="effort === choice" class="ui-menu-check" aria-hidden="true" />
            </button>
          </div>
        </div>
        <div v-if="others.length" class="model-submenu">
          <button
            type="button"
            class="ui-menu-item model-submenu-row"
            aria-haspopup="menu"
            :aria-expanded="submenu === 'more'"
            :data-highlighted="submenu === 'more' ? '' : undefined"
            @mouseenter="openSubmenu('more', $event.currentTarget)"
            @click="toggleSubmenu('more', $event)"
            @keydown.right.prevent="openSubmenu('more', $event.currentTarget)"
          >
            <span>{{ t("composer.moreModels") }}</span>
            <ChevronRight aria-hidden="true" />
          </button>
          <div v-if="submenu === 'more'" class="ui-popover model-submenu-panel model-more" role="menu" :aria-label="t('composer.moreModels')" :data-side="placement.side" :style="placement.side === 'inline' ? undefined : { bottom: `${placement.bottom}px` }">
            <label class="ui-menu-search">
              <Search aria-hidden="true" />
              <input ref="searchInput" v-model="query" type="text" :placeholder="t('composer.searchModels')" :aria-label="t('composer.searchModels')">
            </label>
            <div class="model-more-list">
              <p v-if="groups.length === 0" class="menu-empty">{{ t("composer.noModels") }}</p>
              <template v-for="group in groups" :key="group.providerId">
                <p class="ui-menu-label">{{ group.name }}</p>
                <button
                  v-for="option in group.options"
                  :key="`${option.providerId}:${option.modelId}`"
                  type="button"
                  role="menuitemradio"
                  class="ui-menu-item"
                  :aria-checked="isCurrent(option)"
                  :title="`${option.name} · ${option.providerName}`"
                  @click="select(option)"
                >
                  <span class="model-name">{{ option.name }}</span>
                  <Check v-if="isCurrent(option)" class="ui-menu-check" aria-hidden="true" />
                </button>
              </template>
            </div>
          </div>
        </div>
      </template>
      <hr class="ui-menu-separator">
      <div class="model-actions" @mouseenter="submenu = undefined">
        <button v-if="model && connection.supports('provider.select') && connection.supports('provider.selection')" type="button" class="ui-menu-item" :disabled="savingDefault || connection.phase !== 'connected'" @click="makeDefault"><Star aria-hidden="true" />{{ t("composer.setDefault") }}</button>
        <RouterLink class="ui-menu-item" to="/settings/providers" @click="close"><Settings aria-hidden="true" />{{ t("composer.modelSettings") }}</RouterLink>
      </div>
    </div>
  </details>
</template>

<style scoped>
.model-trigger { max-width: min(300px, 36vw); }
.model-trigger-label { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.model-trigger-effort { flex: none; color: var(--muted-foreground); }
/* Provider initial: a 16px glyph tile on the xs control radius. */
.provider-badge { display: grid; width: 16px; height: 16px; flex: none; place-items: center; border-radius: var(--control-xs-radius); background: var(--muted); color: var(--foreground); font-size: var(--text-xs); font-weight: var(--weight-medium); line-height: 1; }
.model-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
/* Submenus sit outside the menu's box, so the menu itself does not clip; its model list scrolls instead. */
.model-content { display: flex; flex-direction: column; }
.model-list { display: flex; max-height: min(320px, 40dvh); flex-direction: column; gap: 2px; overflow-y: auto; }
.model-list kbd { font-family: var(--font-mono); }
.model-submenu-row > :first-child { flex: 1; }
.model-submenu-value { color: var(--muted-foreground); font-size: var(--text-sm); }
.model-submenu-panel { position: absolute; width: 280px; max-height: min(420px, 70dvh); overflow-y: auto; transform-origin: bottom left; --popover-enter-y: 0px; }
.model-submenu-panel[data-side="right"] { left: calc(100% + 6px); }
.model-submenu-panel[data-side="left"] { right: calc(100% + 6px); transform-origin: bottom right; }
.model-submenu-panel[data-side="inline"] { position: static; width: auto; max-height: 260px; margin: 2px 0 var(--space-1) var(--space-3); border: 0; box-shadow: none; padding: 0; }
.model-submenu-hint { margin: 0; padding: var(--space-1) var(--space-2) var(--space-2); color: var(--muted-foreground); font-size: var(--text-sm); line-height: var(--leading-snug); }
.model-more { display: flex; flex-direction: column; overflow: hidden; }
.model-more-list { display: flex; min-height: 0; flex: 1; flex-direction: column; gap: 2px; overflow-y: auto; }
.model-actions a { color: inherit; text-decoration: none; }
@container composer (max-width: 640px) {
  .model-menu { position: static; }
  .model-content.is-end { right: 0; left: 0; width: auto; max-width: none; transform-origin: bottom center; }
}
</style>
