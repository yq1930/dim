<script setup lang="ts">
import { inject, ref } from "vue"
import { useI18n } from "vue-i18n"
import { File, FolderOpen } from "lucide-vue-next"
import { copyText } from "../../../clipboard"
import { LINK_CONTEXT } from "../../../components/link-context"
import { canChooseWorkspace, openWorkspacePath } from "../../../desktop"
import type { DeliveredFile } from "../../../rpc/protocol"
import { useFilesStore } from "../../../stores/files"
import { useToastStore } from "../../../stores/toast"

defineProps<{ files: readonly DeliveredFile[] }>()
const { t } = useI18n()
const link = inject(LINK_CONTEXT)
const viewer = useFilesStore()
const toast = useToastStore()
const pending = ref<string>()

function name(path: string): string {
  return path.split(/[/\\]/).at(-1) ?? path
}

async function open(path: string, reveal = false): Promise<void> {
  if (pending.value !== undefined) return
  pending.value = path
  try {
    const context = link?.value
    if (!reveal && context?.filePane && viewer.readable(context.sessionId)) {
      if (await viewer.open(context.sessionId, path)) return
    }
    if (canChooseWorkspace) {
      await openWorkspacePath(path, reveal)
    } else {
      await copyText(path)
      toast.push({ tone: "success", title: t("chat.pathCopied") })
    }
  } catch (cause) {
    toast.push({ tone: "danger", title: t("chat.openFailed"), description: String(cause) })
  } finally {
    pending.value = undefined
  }
}
</script>

<template>
  <ul class="turn-deliverables" :aria-label="t('timeline.deliveredFiles')">
    <li v-for="file in files" :key="file.path" class="delivery">
      <button type="button" class="ui-menu-item delivery-file" :title="file.path" :disabled="pending !== undefined" :aria-busy="pending === file.path" @click="open(file.path, $event.metaKey)">
        <File :size="17" aria-hidden="true" />
        <span class="delivery-text">
          <strong>{{ name(file.path) }}</strong>
          <small v-if="file.description">{{ file.description }}</small>
        </span>
      </button>
      <button v-if="canChooseWorkspace" type="button" class="ui-icon-button delivery-reveal" data-size="sm" data-variant="ghost" :aria-label="t('files.reveal')" v-tooltip="t('files.reveal')" :disabled="pending !== undefined" @click="open(file.path, true)">
        <FolderOpen aria-hidden="true" />
      </button>
    </li>
  </ul>
</template>

<style scoped>
.turn-deliverables { display: grid; flex: 0 0 100%; gap: var(--space-2); margin: var(--space-1) 0; padding: 0; list-style: none; }
.delivery { display: flex; min-width: 0; align-items: center; gap: var(--space-1); padding: var(--space-1); border: 1px solid var(--border); border-radius: var(--radius-lg); color: var(--foreground); }
.delivery-file { flex: 1; min-width: 0; height: auto; min-height: var(--control-md); align-items: center; text-align: left; }
.delivery-file > svg { flex: none; color: var(--muted-foreground); }
.delivery-text { display: grid; min-width: 0; }
.delivery-text strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: var(--weight-medium); }
.delivery-text small { overflow: hidden; color: var(--muted-foreground); text-overflow: ellipsis; white-space: nowrap; }
.delivery-reveal { flex: none; }
</style>
