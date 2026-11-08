<script setup lang="ts">
import { useI18n } from "vue-i18n"
import { Check, ListFilter } from "lucide-vue-next"
import RowActionMenu from "../../components/RowActionMenu.vue"
import { useUiStore, type SidebarGroupBy, type SidebarSortBy } from "../../stores/ui"

/** The sidebar's filter menu, as Claude Code's: how Sessions are grouped and sorted. */
const { t } = useI18n()
const ui = useUiStore()
const GROUPS: readonly SidebarGroupBy[] = ["project", "date", "state", "none"]
const SORTS: readonly SidebarSortBy[] = ["recency", "alpha", "created"]
</script>

<template>
  <RowActionMenu :label="t('sidebar.view')" :icon="ListFilter">
    <p class="ui-menu-label">{{ t("sidebar.groupBy") }}</p>
    <button v-for="group in GROUPS" :key="group" type="button" role="menuitemradio" class="ui-menu-item" :aria-checked="ui.sidebarGroupBy === group" @click="ui.sidebarGroupBy = group">
      <span>{{ t(`sidebar.group.${group}`) }}</span>
      <Check v-if="ui.sidebarGroupBy === group" class="ui-menu-check" aria-hidden="true" />
    </button>
    <hr class="ui-menu-separator">
    <p class="ui-menu-label">{{ t("sidebar.sortBy") }}</p>
    <button v-for="sort in SORTS" :key="sort" type="button" role="menuitemradio" class="ui-menu-item" :aria-checked="ui.sidebarSortBy === sort" @click="ui.sidebarSortBy = sort">
      <span>{{ t(`sidebar.sort.${sort}`) }}</span>
      <Check v-if="ui.sidebarSortBy === sort" class="ui-menu-check" aria-hidden="true" />
    </button>
  </RowActionMenu>
</template>
