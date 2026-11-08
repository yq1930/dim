<script setup lang="ts">
import { computed } from "vue"
import { canChooseWorkspace } from "../../desktop"
import { useSettingsText } from "../../i18n/settings-general"
import { useConnectionStore } from "../../stores/connection"
import { usePreferencesStore } from "../../stores/preferences"

const s = useSettingsText()
const connection = useConnectionStore()
const preferences = usePreferencesStore()
const archivable = computed(() => ["git.pr", "git.status", "session.archive"].every((method) => connection.supports(method)))
</script>

<template>
  <article class="settings-page">
    <header><h2 data-route-heading tabindex="-1">{{ s('General', '通用') }}</h2><p>{{ s('Changes apply immediately and are saved in this browser.', '更改立即生效，保存在此浏览器中。') }}</p></header>
    <section v-if="canChooseWorkspace" class="settings-card settings-toggle">
      <div>
        <h3 id="keep-awake">{{ s('Keep the computer awake while sessions run', '运行时防止休眠') }}</h3>
        <p>{{ s('While a session runs, the computer does not sleep on its own, so long tasks can finish. The display can still turn off, and closing the lid still sleeps the computer.', '有会话运行时，电脑不会自动休眠，长任务可以跑完。显示器仍会关闭，合上笔记本盖子仍会休眠。') }}</p>
      </div>
      <button type="button" role="switch" class="ui-switch" :aria-checked="preferences.keepAwake" aria-labelledby="keep-awake" @click="preferences.keepAwake = !preferences.keepAwake" />
    </section>
    <section v-if="archivable" class="settings-card settings-toggle">
      <div>
        <h3 id="auto-archive">{{ s('Archive sessions when their pull request merges or closes', 'PR 合并或关闭后自动归档') }}</h3>
        <p>{{ s('Archives the sessions on a branch once its open pull request merges or closes; after a merge, a worktree Lacus created is removed and its branch kept. Sessions with uncommitted changes stay.', '分支上打开的 PR 合并或关闭后，归档在该分支上工作的会话；合并后还会移除 Lacus 创建的工作树，分支保留。有未提交改动的会话不会归档。') }}</p>
      </div>
      <button type="button" role="switch" class="ui-switch" :aria-checked="preferences.autoArchive" aria-labelledby="auto-archive" @click="preferences.autoArchive = !preferences.autoArchive" />
    </section>
  </article>
</template>

<style scoped>
.settings-toggle { display: flex; align-items: flex-start; gap: var(--space-4); }
.settings-toggle > div { min-width: 0; flex: 1; }
.settings-toggle p { margin: var(--space-2) 0 0; color: var(--muted-foreground); font-size: var(--text-sm); line-height: var(--leading-normal); }
.settings-toggle > .ui-switch { flex: none; margin-top: 2px; }
</style>
