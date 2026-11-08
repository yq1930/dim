<script setup lang="ts">
import { computed, onMounted, ref } from "vue"
import { useI18n } from "vue-i18n"
import { ChevronDown, Copy, Folder, SquareCode, SquareTerminal } from "lucide-vue-next"
import RowActionMenu from "../../../components/RowActionMenu.vue"
import { copyText } from "../../../clipboard"
import { installedApps, openInApp, type DesktopApp } from "../../../desktop"
import { usePreferencesStore } from "../../../stores/preferences"
import { useToastStore } from "../../../stores/toast"

/**
 * Opens the Session's folder in an editor, a terminal or Finder, as Claude
 * Code's "Open in" does. The button opens the app last chosen; the menu lists
 * every app the desktop app found. Outside the desktop app it shows nothing.
 */
const props = defineProps<{ path: string }>()
const { t } = useI18n()
const preferences = usePreferencesStore()
const toast = useToastStore()
const apps = ref<DesktopApp[]>([])
onMounted(async () => { apps.value = await installedApps() })

const KIND_ICON = { editor: SquareCode, terminal: SquareTerminal, finder: Folder } as const
const preferred = computed(() => apps.value.find((app) => app.id === preferences.openWith) ?? apps.value.find((app) => app.kind === "editor") ?? apps.value[0])
/** Menu sections: editors, then terminals, then Finder. */
const groups = computed(() => (["editor", "terminal", "finder"] as const).map((kind) => apps.value.filter((app) => app.kind === kind)).filter((group) => group.length > 0))

function name(app: DesktopApp): string {
  if (app.kind === "finder") return t("openIn.finder")
  return app.id === "com.apple.Terminal" ? t("openIn.terminal") : app.name
}

async function open(app: DesktopApp): Promise<void> {
  preferences.openWith = app.id
  try {
    await openInApp(app.id, props.path)
  } catch (cause) {
    toast.push({ tone: "danger", title: t("openIn.failed", { app: name(app) }), description: cause instanceof Error ? cause.message : String(cause) })
  }
}

async function copyPath(): Promise<void> {
  await copyText(props.path)
  toast.push({ tone: "success", title: t("chat.pathCopied"), description: props.path })
}
</script>

<template>
  <span v-if="preferred" class="open-in" role="group" :aria-label="t('openIn.choose')">
    <button type="button" class="ui-icon-button open-in-app" data-variant="ghost" data-size="sm" :aria-label="t('openIn.app', { app: name(preferred) })" v-tooltip="t('openIn.app', { app: name(preferred) })" @click="open(preferred)">
      <img v-if="preferred.icon" :src="preferred.icon" alt="" class="open-in-icon"><component :is="KIND_ICON[preferred.kind]" v-else aria-hidden="true" />
    </button>
    <RowActionMenu :label="t('openIn.choose')" :icon="ChevronDown" trigger-class="open-in-more">
      <template v-for="(group, index) in groups" :key="group[0]!.kind">
        <hr v-if="index > 0" class="ui-menu-separator">
        <button v-for="app in group" :key="app.id" type="button" class="ui-menu-item open-in-item" @click="open(app)">
          <img v-if="app.icon" :src="app.icon" alt="" class="open-in-icon"><component :is="KIND_ICON[app.kind]" v-else aria-hidden="true" />
          <span>{{ name(app) }}</span>
        </button>
      </template>
      <hr class="ui-menu-separator">
      <button type="button" class="ui-menu-item" @click="copyPath"><Copy aria-hidden="true" />{{ t("files.copyPath") }}</button>
    </RowActionMenu>
  </span>
</template>

<style scoped>
.open-in { display: inline-flex; flex: none; align-items: center; }
.open-in-icon { width: 16px; height: 16px; flex: none; }
.open-in :deep(.open-in-more) { width: 18px; margin-left: -2px; }
.open-in :deep(.open-in-more) > svg { width: 12px; height: 12px; }
</style>
