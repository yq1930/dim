<script setup lang="ts">
import { computed } from "vue"
import { useI18n } from "vue-i18n"
import { Check, Star } from "lucide-vue-next"
import RowActionMenu from "../../components/RowActionMenu.vue"
import { TRANSCRIPT_VIEWS, usePreferencesStore } from "../../stores/preferences"

/** The header's ⋯ menu: the Session's transcript view, as Claude Code's Transcript view menu. */
const props = defineProps<{ sessionId: string }>()
const { t } = useI18n()
const preferences = usePreferencesStore()
const current = computed(() => preferences.viewOf(props.sessionId))
const cycleKey = navigator.userAgent.includes("Mac") ? "⌃O" : "Ctrl+O"
</script>

<template>
  <RowActionMenu :label="t('session.menu')">
    <p class="ui-menu-label transcript-label">{{ t("transcript.title") }}<kbd class="ui-menu-meta">{{ cycleKey }}</kbd></p>
    <button v-for="mode in TRANSCRIPT_VIEWS" :key="mode" type="button" role="menuitemradio" class="ui-menu-item" :aria-checked="current === mode" @click="preferences.setView(sessionId, mode)">
      <span>{{ t(`transcript.${mode}`) }}</span>
      <Check v-if="current === mode" class="ui-menu-check" aria-hidden="true" />
    </button>
    <template v-if="current !== preferences.transcriptView">
      <hr class="ui-menu-separator">
      <button type="button" class="ui-menu-item" @click="preferences.makeDefault(sessionId)"><Star aria-hidden="true" />{{ t("transcript.makeDefault", { view: t(`transcript.${current}`) }) }}</button>
    </template>
  </RowActionMenu>
</template>

<style scoped>
.transcript-label { display: flex; align-items: center; }
</style>
