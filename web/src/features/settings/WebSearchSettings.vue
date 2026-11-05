<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue"
import { useSettingsText } from "../../i18n/settings-general"
import { readWebSearchSettings, type WebSearchSettings } from "../../rpc/general-settings"
import { useConnectionStore } from "../../stores/connection"
import GeneralSettingsStatus from "./GeneralSettingsStatus.vue"
import { bindSettingsDirty, messageFrom, reloadSettingsOnReconnect } from "./context"

const ACCOUNT = "dim-oauth"
const services = [
  { kind: "deepseek", label: "DeepSeek", endpoint: "https://api.deepseek.com/v1/tools/search" },
  { kind: "tavily", label: "Tavily", endpoint: "https://api.tavily.com/search" },
  { kind: "brave", label: "Brave Search", endpoint: "https://api.search.brave.com/res/v1/web/search" },
]

const s = useSettingsText()
const connection = useConnectionStore()
const snapshot = ref<WebSearchSettings>()
const kind = ref(ACCOUNT)
const apiKey = ref("")
const endpoint = ref("")
const proxy = ref("")
const clearProxy = ref(false)
const busy = ref(false)
const error = ref("")
const saved = ref(false)

const storedKind = computed(() => snapshot.value?.kind ?? ACCOUNT)
const usesKey = computed(() => kind.value !== ACCOUNT)
const keepsKey = computed(() => kind.value === storedKind.value && snapshot.value?.keySet === true)
const defaultEndpoint = computed(() => services.find((service) => service.kind === kind.value)?.endpoint ?? "")
const dirty = computed(() => snapshot.value !== undefined && (
  kind.value !== storedKind.value || apiKey.value !== "" || endpoint.value.trim() !== snapshot.value.endpoint || proxy.value.trim() !== "" || clearProxy.value
))
const problem = computed(() => {
  if (usesKey.value && apiKey.value === "" && !keepsKey.value) return s("Enter the service's API key.", "请填写该服务的 API Key。")
  for (const url of [usesKey.value ? endpoint.value.trim() : "", proxy.value.trim()]) {
    if (url !== "" && !URL.canParse(url)) return s("Enter a complete URL, including http:// or https://.", "请填写完整的网址，包含 http:// 或 https://。")
  }
  return ""
})
bindSettingsDirty(dirty)
watch(kind, (next) => {
  apiKey.value = ""
  endpoint.value = next === storedKind.value ? snapshot.value?.endpoint ?? "" : ""
})

function apply(next: WebSearchSettings): void {
  snapshot.value = next
  kind.value = next.kind ?? ACCOUNT
  apiKey.value = ""
  endpoint.value = next.endpoint
  proxy.value = ""
  clearProxy.value = false
}
async function load(): Promise<void> {
  if (busy.value) return
  if (dirty.value && !window.confirm(s("Discard unsaved search settings?", "放弃未保存的搜索设置？"))) return
  busy.value = true
  error.value = ""
  saved.value = false
  try {
    if (!connection.supports("settings.view")) throw new Error(s("This server cannot change search settings.", "此服务器不支持修改搜索设置。"))
    apply(await connection.client.call("settings.view", { source: "web", scope: "user" }, readWebSearchSettings))
  } catch (cause) { error.value = messageFrom(cause) }
  finally { busy.value = false }
}
async function save(): Promise<void> {
  const current = snapshot.value
  if (!current || !dirty.value || problem.value !== "" || busy.value) return
  busy.value = true
  error.value = ""
  saved.value = false
  try {
    const { searchService: _service, ...kept } = current.document
    const url = endpoint.value.trim()
    const typedProxy = proxy.value.trim()
    const document = {
      ...kept,
      ...(usesKey.value ? { searchService: { kind: kind.value, ...(apiKey.value === "" ? {} : { apiKey: apiKey.value }), ...(url === "" ? {} : { endpoint: url }) } } : {}),
      ...(typedProxy === "" ? {} : { proxy: typedProxy }),
    }
    const clear = clearProxy.value && typedProxy === "" ? ["/proxy"] : []
    apply(await connection.client.call("settings.edit", { source: "web", scope: "user", expectedRevision: current.revision, document, clear }, readWebSearchSettings))
    saved.value = true
  } catch (cause) { error.value = messageFrom(cause) }
  finally { busy.value = false }
}
reloadSettingsOnReconnect(connection, load, dirty)
onMounted(load)
</script>

<template>
  <article class="settings-page">
    <header><h2 data-route-heading tabindex="-1">{{ s('Web search', '网络搜索') }}</h2></header>
    <GeneralSettingsStatus :busy="busy" :error="error" :saved="saved && !dirty" @reload="load" />
    <form v-if="snapshot" class="settings-card settings-form" @submit.prevent="save">
      <fieldset class="settings-form" :disabled="busy || !connection.supports('settings.update')">
        <label>
          <span>{{ s('Search service', '搜索服务') }}</span>
          <select v-model="kind" class="ui-field">
            <option :value="ACCOUNT">{{ s('Dim account', 'Dim 账号') }}</option>
            <option v-for="service in services" :key="service.kind" :value="service.kind">{{ service.label }}</option>
            <option v-if="storedKind !== ACCOUNT && !services.some((service) => service.kind === storedKind)" :value="storedKind">{{ storedKind }}</option>
          </select>
        </label>
        <template v-if="usesKey">
          <label>
            <span>API Key</span>
            <input v-model="apiKey" class="ui-field" type="password" autocomplete="new-password" :placeholder="keepsKey ? s('Saved. Leave empty to keep it.', '已保存，留空则保持不变') : ''" />
          </label>
          <label>
            <span>{{ s('Endpoint', '接口地址') }}</span>
            <input v-model="endpoint" class="ui-field" type="url" spellcheck="false" :placeholder="defaultEndpoint" />
          </label>
        </template>
        <label>
          <span>{{ s('Proxy', '代理') }}</span>
          <input v-model="proxy" class="ui-field" type="url" spellcheck="false" :placeholder="snapshot.proxySet && !clearProxy ? s('Saved. Leave empty to keep it.', '已保存，留空则保持不变') : 'http://127.0.0.1:7890'" />
        </label>
        <label v-if="snapshot.proxySet"><input v-model="clearProxy" type="checkbox" />{{ s('Remove the saved proxy', '删除已保存的代理') }}</label>
      </fieldset>
      <p v-if="dirty && problem" class="ui-banner settings-banner" data-tone="warning" role="alert">{{ problem }}</p>
      <div class="settings-actions"><button class="ui-button" data-variant="primary" type="submit" :disabled="busy || !dirty || problem !== ''">{{ s('Save', '保存') }}</button></div>
    </form>
  </article>
</template>
