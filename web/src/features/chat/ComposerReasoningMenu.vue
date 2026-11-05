<script setup lang="ts">
import { useI18n } from "vue-i18n"

defineProps<{ choices: string[]; value?: string; disabled: boolean }>()
const emit = defineEmits<{ select: [effort: string | undefined] }>()
const { t } = useI18n()
const labels: Record<string, string> = {
  none: "composer.effortNone", minimal: "composer.effortMinimal", low: "composer.effortLow",
  medium: "composer.effortMedium", high: "composer.effortHigh", xhigh: "composer.effortXhigh",
  max: "composer.effortMax", ultra: "composer.effortUltra",
}
function label(value: string): string { return labels[value] ? t(labels[value]!) : value }
</script>

<template>
  <label class="reasoning-control">
    <span>{{ t("composer.reasoningEffort") }}</span>
    <select class="ui-field" data-size="sm" :aria-label="t('composer.reasoningEffort')" :value="value ?? ''" :disabled="disabled" @change="emit('select', ($event.target as HTMLSelectElement).value || undefined)">
      <option value="">{{ t("composer.effortDefault") }}</option>
      <option v-if="value && !choices.includes(value)" :value="value" disabled>{{ label(value) }}</option>
      <option v-for="choice in choices" :key="choice" :value="choice">{{ label(choice) }}</option>
    </select>
  </label>
</template>

<style scoped>
.reasoning-control { display: flex; flex: none; align-items: center; gap: var(--space-1); font-size: var(--text-sm); white-space: nowrap; }
.reasoning-control .ui-field { width: auto; min-width: 64px; max-width: 120px; }
</style>
