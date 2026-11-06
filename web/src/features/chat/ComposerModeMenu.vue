<script lang="ts">
import type { PermissionPreset } from "../../rpc/protocol"

/** How the line works: a permission preset, or plan mode over one. */
export type WorkMode = PermissionPreset | "plan"
</script>

<script setup lang="ts">
import { computed, ref, type Component } from "vue"
import { RouterLink } from "vue-router"
import { useI18n } from "vue-i18n"
import { Check, ChevronDown, Eye, Hand, ListChecks, PencilLine, Zap } from "lucide-vue-next"
import ConfirmDialog from "../../components/ConfirmDialog.vue"
import type { PermissionPolicy } from "../../rpc/protocol"

const props = defineProps<{
  /** The modes offered, in menu order. */
  modes: WorkMode[]
  current: WorkMode | undefined
  /** The Session policy, for its tool lists; absent before a Session exists. */
  policy?: PermissionPolicy
  disabled: boolean
}>()
const emit = defineEmits<{ select: [mode: WorkMode] }>()
const { t } = useI18n()
const details = ref<HTMLDetailsElement>()
const confirm = ref<InstanceType<typeof ConfirmDialog>>()

const ICONS: Record<WorkMode, Component> = {
  "ask": Hand,
  "workspace-write": PencilLine,
  "full-access": Zap,
  "plan": ListChecks,
  "read-only": Eye,
}
const icon = computed(() => props.current === undefined ? Hand : ICONS[props.current])
/** The composer's Shift+Tab cycles Ask, Accept edits and Plan where two of them are offered. */
const cycles = computed(() => props.modes.filter((mode) => mode === "ask" || mode === "workspace-write" || mode === "plan").length > 1)

/** The confirmation lives inside the menu, so the menu closes only once it is answered. */
async function choose(mode: WorkMode): Promise<void> {
  const confirmed = mode === props.current || mode !== "full-access" || await confirm.value?.open(t("composer.mode.autoConfirm"))
  if (details.value) details.value.open = false
  if (confirmed && mode !== props.current) emit("select", mode)
}

defineExpose({ open: () => {
  if (!props.disabled && details.value) details.value.open = true
} })
</script>

<template>
  <details ref="details" class="composer-popover work-mode-menu" :class="{ 'is-disabled': disabled }">
    <summary class="ui-button work-mode-trigger" data-variant="ghost" data-size="sm" :data-mode="current" :aria-label="t('composer.mode.label')">
      <component :is="icon" aria-hidden="true" />
      <span>{{ current === undefined ? t("composer.mode.label") : t(`composer.mode.${current}`) }}</span>
      <ChevronDown class="work-mode-chevron" aria-hidden="true" />
    </summary>
    <div class="composer-popover-content ui-popover work-mode-content" data-width="lg" role="menu" :aria-label="t('composer.mode.label')">
      <button
        v-for="mode in modes"
        :key="mode"
        type="button"
        role="menuitemradio"
        class="ui-menu-item"
        data-two-line
        :aria-checked="mode === current"
        @click="choose(mode)"
      >
        <component :is="ICONS[mode]" aria-hidden="true" />
        <span class="work-mode-text"><span>{{ t(`composer.mode.${mode}`) }}</span><small class="ui-menu-desc">{{ t(`composer.mode.description.${mode}`) }}</small></span>
        <Check v-if="mode === current" class="ui-menu-check" aria-hidden="true" />
      </button>
      <template v-if="policy?.allowedTools.length || policy?.disallowedTools.length">
        <hr class="ui-menu-separator">
        <p v-if="policy?.allowedTools.length" class="work-mode-note">{{ t("composer.mode.allowedTools") }}: {{ policy.allowedTools.join(", ") }}</p>
        <p v-if="policy?.disallowedTools.length" class="work-mode-note">{{ t("composer.mode.disallowedTools") }}: {{ policy.disallowedTools.join(", ") }}</p>
      </template>
      <template v-if="policy">
        <hr class="ui-menu-separator">
        <RouterLink class="ui-menu-item work-mode-link" :to="{ name: 'settings-permissions' }" @click="details && (details.open = false)">{{ t("composer.mode.processIsolation") }}</RouterLink>
      </template>
      <p v-if="cycles" class="work-mode-keys">{{ t("composer.mode.cycleKeys") }}</p>
    </div>
    <ConfirmDialog ref="confirm" />
  </details>
</template>

<style scoped>
.work-mode-trigger { gap: var(--space-1); color: var(--muted-foreground); }
.work-mode-trigger > svg { width: 14px; height: 14px; }
.work-mode-trigger[data-mode="plan"] { background: var(--accent); color: var(--accent-foreground); }
.work-mode-chevron { transition: transform var(--duration-press) var(--ease-out); }
.work-mode-menu[open] .work-mode-chevron { transform: rotate(180deg); }
.work-mode-content { display: grid; gap: 2px; }
.work-mode-text { display: grid; min-width: 0; flex: 1; }
.work-mode-note { margin: 0; padding: var(--space-1) var(--space-2); color: var(--muted-foreground); font-size: var(--text-xs); overflow-wrap: anywhere; }
.work-mode-link { color: var(--muted-foreground); font-size: var(--text-sm); }
.work-mode-keys { margin: var(--space-1) 0 0; padding: var(--space-2) var(--space-2) var(--space-1); border-top: 1px solid var(--border); color: var(--muted-foreground); font-size: var(--text-xs); }
</style>
