<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from "vue"
import { useI18n } from "vue-i18n"
import { RouterLink } from "vue-router"
import { Check, LoaderCircle, Search, Settings, Star } from "lucide-vue-next"
import type { ModelId } from "../../rpc/protocol"
import { useModelsStore } from "../../stores/models"
import { useConnectionStore } from "../../stores/connection"
import { useToastStore } from "../../stores/toast"

/**
 * Provider and model picker: a popover with search, a provider pane and a model
 * pane. When the model cannot be changed (no provider methods, or no
 * `session.model.update` on this connection) it only names the current model.
 */
type Option = { providerId: string; providerName: string; modelId: string; name: string }

const props = defineProps<{ model: ModelId | undefined; selectable: boolean; disabled: boolean }>()
const emit = defineEmits<{ select: [model: ModelId] }>()
const { t } = useI18n()
const models = useModelsStore()
const connection = useConnectionStore()
const savingDefault = ref(false)
const details = ref<HTMLDetailsElement>()
const searchInput = ref<HTMLInputElement>()
const query = ref("")
const selectedProvider = ref<string>()
const activeIndex = ref(0)

const providers = computed(() => models.providers ?? [])
const currentProvider = computed(() => providers.value.find((provider) => provider.providerId === selectedProvider.value)?.providerId
  ?? providers.value.find((provider) => provider.providerId === props.model?.providerId)?.providerId
  ?? providers.value[0]?.providerId)
const allOptions = computed<Option[]>(() => providers.value.flatMap((provider) => provider.models.map((model) => ({
  providerId: provider.providerId,
  providerName: provider.name,
  modelId: model.id,
  name: model.name,
}))))
const searching = computed(() => query.value.trim() !== "")
const paneOptions = computed(() => {
  if (!searching.value) return allOptions.value.filter((option) => option.providerId === currentProvider.value)
  const needle = query.value.trim().toLocaleLowerCase()
  return allOptions.value.filter((option) => [option.name, option.modelId, option.providerName].some((value) => value.toLocaleLowerCase().includes(needle)))
})
const triggerLabel = computed(() => props.model === undefined ? t("composer.notConfigured") : models.nameOf(props.model))

// The trigger names the model from the catalog once it is loaded.
onMounted(() => {
  if (props.selectable && models.available && models.providers === undefined) void models.load()
})

function isCurrent(option: Option): boolean {
  return option.providerId === props.model?.providerId && option.modelId === props.model.modelId
}

function initial(name: string): string {
  return name.trim().charAt(0).toLocaleUpperCase()
}

async function onToggle(): Promise<void> {
  if (details.value?.open !== true) return
  if (models.providers === undefined) await models.load()
  query.value = ""
  selectedProvider.value = props.model?.providerId ?? providers.value[0]?.providerId
  activeIndex.value = Math.max(0, paneOptions.value.findIndex(isCurrent))
  await nextTick()
  if (!matchMedia("(pointer: coarse)").matches) searchInput.value?.focus()
}

function select(option: Option): void {
  if (details.value) details.value.open = false
  if (!isCurrent(option)) emit("select", { providerId: option.providerId, modelId: option.modelId })
}

async function makeDefault(): Promise<void> {
  const model = props.model
  if (!model || savingDefault.value) return
  savingDefault.value = true
  try {
    await models.setGlobalDefault(model)
    if (details.value) details.value.open = false
    useToastStore().push({ tone: "success", title: t("composer.defaultSaved", { model: models.nameOf(model) }) })
  } catch (cause) { useToastStore().push({ tone: "danger", title: t("composer.defaultFailed"), description: cause instanceof Error ? cause.message : String(cause) }) }
  finally { savingDefault.value = false }
}

function onSearchKey(event: KeyboardEvent): void {
  if (event.isComposing || event.keyCode === 229) return
  if (event.key === "Escape") {
    event.preventDefault()
    if (details.value) details.value.open = false
    details.value?.querySelector("summary")?.focus()
    return
  }
  if (event.key === "ArrowDown") {
    event.preventDefault()
    activeIndex.value = Math.min(paneOptions.value.length - 1, activeIndex.value + 1)
  } else if (event.key === "ArrowUp") {
    event.preventDefault()
    activeIndex.value = Math.max(0, activeIndex.value - 1)
  } else if (event.key === "Enter") {
    event.preventDefault()
    const option = paneOptions.value[activeIndex.value]
    if (option !== undefined) select(option)
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
    <summary class="ui-button model-trigger" data-variant="ghost" data-size="sm" v-tooltip="model ? `${triggerLabel} · ${model.providerId}` : undefined">
      <span v-if="model" class="provider-badge" aria-hidden="true">{{ initial(model.providerId) }}</span>
      <span class="model-trigger-label">{{ triggerLabel }}</span>
    </summary>
    <div class="composer-popover-content is-end ui-popover model-content" data-width="xl">
      <label class="ui-menu-search model-search">
        <Search aria-hidden="true" />
        <input ref="searchInput" v-model="query" type="text" :placeholder="t('composer.searchModels')" :aria-label="t('composer.searchModels')" @keydown="onSearchKey" @input="activeIndex = 0">
      </label>
      <p v-if="models.providers === undefined && !models.error" class="menu-empty"><LoaderCircle class="is-spinning" aria-hidden="true" />{{ t("composer.modelLoading") }}</p>
      <p v-else-if="models.error" class="menu-empty">
        <button type="button" class="ui-button" data-variant="danger-ghost" data-size="sm" @click="models.load()">{{ t("composer.modelLoadRetry") }}</button>
      </p>
      <div v-if="models.providers !== undefined && (!models.error || providers.length > 0)" class="model-panes">
        <div v-if="!searching" class="provider-pane">
          <button v-for="provider in providers" :key="provider.providerId" type="button" class="ui-menu-item" :aria-current="currentProvider === provider.providerId" @click="selectedProvider = provider.providerId; activeIndex = 0">
            <span class="provider-badge" aria-hidden="true">{{ initial(provider.name) }}</span><span>{{ provider.name }}</span>
          </button>
        </div>
        <div class="model-pane" role="listbox" :aria-label="t('composer.searchModels')">
          <p v-if="paneOptions.length === 0" class="menu-empty">{{ t("composer.noModels") }}</p>
          <button
            v-for="(option, index) in paneOptions"
            :key="`${option.providerId}:${option.modelId}`"
            type="button"
            role="option"
            class="ui-menu-item"
            :aria-selected="isCurrent(option)"
            :data-highlighted="index === activeIndex ? '' : undefined"
            :title="`${option.name} · ${option.providerName}`"
            @mouseenter="activeIndex = index"
            @click="select(option)"
          >
            <span class="model-check"><Check v-if="isCurrent(option)" aria-hidden="true" /></span>
            <span v-if="searching" class="provider-badge" aria-hidden="true">{{ initial(option.providerName) }}</span>
            <span class="model-name">{{ option.name }}</span>
          </button>
        </div>
      </div>
      <div class="model-actions">
        <button v-if="model && connection.supports('provider.select') && connection.supports('provider.selection')" type="button" class="ui-menu-item" :disabled="savingDefault || connection.phase !== 'connected'" @click="makeDefault"><Star aria-hidden="true" />{{ t("composer.setDefault") }}</button>
        <RouterLink class="ui-menu-item" to="/settings/providers" @click="details && (details.open = false)"><Settings aria-hidden="true" />{{ t("composer.modelSettings") }}</RouterLink>
      </div>
    </div>
  </details>
</template>

<style scoped>
.model-trigger { max-width: min(260px, 32vw); }
.model-trigger-label { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
/* Provider initial: a 16px glyph tile on the xs control radius. */
.provider-badge { display: grid; width: 16px; height: 16px; flex: none; place-items: center; border-radius: var(--control-xs-radius); background: var(--muted); color: var(--foreground); font-size: var(--text-xs); font-weight: var(--weight-medium); line-height: 1; }
.model-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.model-content { display: flex; max-height: 45dvh; flex-direction: column; overflow: hidden; }
.model-panes { display: flex; height: 240px; min-height: 96px; margin-inline: calc(-1 * var(--space-1)); }
.provider-pane { display: flex; width: 132px; flex: none; flex-direction: column; gap: 2px; overflow-y: auto; padding: 0 var(--space-1); border-right: 1px solid var(--border); }
.model-pane { display: flex; min-width: 0; flex: 1; flex-direction: column; gap: 2px; overflow-y: auto; padding: 0 var(--space-1); }
.model-check { display: flex; width: 14px; height: 14px; flex: none; align-items: center; justify-content: center; color: var(--primary-ink); }
.model-check svg { width: 14px; height: 14px; }
.model-actions { border-top: 1px solid var(--border); margin-top: var(--space-1); padding-top: var(--space-1); }
.model-actions a { color: inherit; text-decoration: none; }
@container composer (max-width: 640px) {
  .model-menu { position: static; }
  .model-content.is-end { right: 0; left: 0; width: auto; max-width: none; transform-origin: bottom center; }
}
</style>
