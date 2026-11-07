<script setup lang="ts">
import { useSettingsText } from "../../i18n/settings-general"
import type { InstallSource } from "./install-source"

/** The fields of an install source, for a settings form: its kind, location, Git ref and installation id. */
defineProps<{ source: InstallSource; pathPlaceholder: string; gitPlaceholder: string; idPlaceholder?: string }>()
const s = useSettingsText()
</script>

<template>
  <label><span>{{ s('Source type', '来源类型') }}</span><select v-model="source.type" class="ui-field" data-size="md"><option value="path">{{ s('Local directory', '本地目录') }}</option><option value="git">Git</option></select></label>
  <label><span>{{ s('Source', '来源') }}</span><input v-model="source.location" class="ui-field" data-size="md" required :placeholder="source.type === 'path' ? pathPlaceholder : gitPlaceholder" /></label>
  <label v-if="source.type === 'git'"><span>{{ s('Git ref (optional)', 'Git 引用（可选）') }}</span><input v-model="source.ref" class="ui-field" data-size="md" /></label>
  <label><span>{{ s('Installation ID (optional)', '安装 ID（可选）') }}</span><input v-model="source.id" class="ui-field" data-size="md" :placeholder="idPlaceholder" /></label>
</template>
