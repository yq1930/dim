import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { registerHooks, stripTypeScriptTypes } from "node:module"
import test from "node:test"
import { compileScript, parse } from "vue/compiler-sfc"
import { createRenderer, nextTick, reactive } from "vue"

registerHooks({
  resolve(specifier, context, next) {
    if (context.parentURL?.endsWith("McpSettings.vue")) {
      if (specifier === "../../stores/connection") return { url: "data:text/javascript,export const useConnectionStore=()=>globalThis.settingsConnection", shortCircuit: true }
    }
    if (specifier === "vue-i18n") return { url: "data:text/javascript,export const useI18n=()=>({locale:{value:'en'}})", shortCircuit: true }
    if (specifier.endsWith(".vue") && !specifier.endsWith("McpSettings.vue")) return { url: "data:text/javascript,export default { props:['busy','error','saved'], emits:['reload'], setup(props){return ()=>props.error||null} }", shortCircuit: true }
    try { return next(specifier, context) } catch (error) {
      if ((specifier.startsWith("./") || specifier.startsWith("../")) && !/\.(ts|vue)$/.test(specifier)) return next(`${specifier}.ts`, context)
      throw error
    }
  },
  load(url, context, next) {
    if (url.endsWith(".ts")) return { format: "module", source: stripTypeScriptTypes(readFileSync(new URL(url), "utf8"), { mode: "transform" }), shortCircuit: true }
    if (!url.endsWith("McpSettings.vue")) return next(url, context)
    const { descriptor } = parse(readFileSync(new URL(url), "utf8"))
    return { format: "module", source: stripTypeScriptTypes(compileScript(descriptor, { id: "settings-mcp-test", inlineTemplate: true }).content, { mode: "transform" }), shortCircuit: true }
  },
})

globalThis.Document = class {}
globalThis.ShadowRoot = class {}

const { readMcp, readMcpConfiguration } = await import("../src/rpc/general-settings.ts")
const { default: McpSettings } = await import("../src/features/settings/McpSettings.vue")

const node = (type, text = "") => ({
  type, text, children: [], props: {}, parent: undefined, value: "", options: [], selectedIndex: -1,
  addEventListener() {}, reportValidity: () => true, getRootNode() { return {} },
})
const renderer = createRenderer({
  createElement: node, createText: (text) => node("text", text), createComment: (text) => node("comment", text),
  patchProp: (element, key, _, value) => { element.props[key] = value },
  setText: (element, text) => { element.text = text },
  setElementText: (element, text) => { element.text = text; element.children = [] },
  parentNode: (element) => element.parent,
  nextSibling: (element) => element.parent?.children[element.parent.children.indexOf(element) + 1],
  insert(element, parent, anchor) {
    if (element.parent) element.parent.children.splice(element.parent.children.indexOf(element), 1)
    parent.children.splice(anchor ? parent.children.indexOf(anchor) : parent.children.length, 0, element)
    element.parent = parent
  },
  remove(element) {
    if (element.parent) element.parent.children.splice(element.parent.children.indexOf(element), 1)
    element.parent = undefined
  },
})
const text = (element) => (element.type === "comment" ? "" : element.text) + element.children.map(text).join("")
const all = (element) => [element, ...element.children.flatMap(all)]
const flush = async () => { for (let i = 0; i < 12; i++) { await Promise.resolve(); await nextTick() } }
const button = (root, label) => all(root).find((element) => element.type === "button" && text(element) === label)
const field = (root, label) => all(root).find((element) => element.type === "label" && text(element).startsWith(label)).children.find((element) => element.type === "input" || element.type === "select")
const enter = (element, value) => {
  if (element.props["onUpdate:modelValue"]) element.props["onUpdate:modelValue"](value)
  else element.props.onChange({ target: { value } })
}
const submit = (root) => all(root).find((element) => element.type === "form" && text(element).includes("Save server")).props.onSubmit({ preventDefault() {} })

function catalog(invalid) {
  return {
    userRevision: "user:1", projectRevision: null,
    userApplication: { error: null, cleanupError: null }, pluginApplication: null, projectApplication: null,
    servers: [{ id: "docs", source: "user", enabled: true, transport: "stdio" }],
    ...(invalid === undefined ? {} : { invalid }),
  }
}

test("a broken entry is read apart from the servers", () => {
  const error = "MCP server broken: field type must be http or sse; ws is not a supported transport"
  const view = readMcp(catalog([{ id: "broken", source: "project", error }]))
  assert.deepEqual(view.servers.map((server) => server.id), ["docs"])
  assert.deepEqual(view.invalid, [{ id: "broken", source: "project", error }])
})

test("a reply without the invalid list still reads its servers", () => {
  const view = readMcp(catalog(undefined))
  assert.deepEqual(view.servers.map((server) => server.id), ["docs"])
  assert.deepEqual(view.invalid, [])
})

test("settings show referenced and unset variable names", () => {
  const view = readMcp({
    ...catalog(undefined),
    servers: [{ id: "docs", source: "user", enabled: true, transport: "http",
      referencedVariables: ["MCP_HOST", "MCP_TOKEN"], unsetVariables: ["MCP_TOKEN"] }],
  })
  assert.deepEqual(view.servers[0].referencedVariables, ["MCP_HOST", "MCP_TOKEN"])
  assert.deepEqual(view.servers[0].unsetVariables, ["MCP_TOKEN"])
  const legacy = readMcp(catalog(undefined))
  assert.deepEqual(legacy.servers[0].referencedVariables, [])
  assert.deepEqual(legacy.servers[0].unsetVariables, [])
})

test("a configuration view names variables without values", () => {
  const configuration = readMcpConfiguration({
    type: "http", endpoint: "https://${MCP_HOST}", headerNames: ["Authorization"],
    authConfigured: true, oauthConfigured: false, oauth: null,
    referencedVariables: ["MCP_HOST", "MCP_TOKEN"], unsetVariables: ["MCP_TOKEN"],
  })
  assert.deepEqual(configuration.referencedVariables, ["MCP_HOST", "MCP_TOKEN"])
  assert.deepEqual(configuration.unsetVariables, ["MCP_TOKEN"])
  const legacy = readMcpConfiguration({ type: "stdio", command: "server", argsCount: 0, cwd: null, envNames: [] })
  assert.deepEqual(legacy.referencedVariables, [])
  assert.deepEqual(legacy.unsetVariables, [])
})

const { configuration, patch } = await import("../src/features/settings/mcp-form.ts")

function draft(overrides) {
  return {
    id: "docs", source: "user", type: "stdio", command: "server", args: [], cwd: "",
    argsAction: "replace", storedArgsCount: 0, secrets: [], enabled: false, token: "",
    authConfigured: false, bearerAction: "keep", oauth: false, oauthAction: "keep",
    clientId: "", callbackPort: "", scopes: "", metadataUrl: "", timeout: "",
    metadataConfigured: false, metadataAction: "keep", secretsInUserScope: false,
    ...overrides,
  }
}

test("a number-typed timeout saves and tests as the number", () => {
  const typed = draft({ timeout: 5000 })
  assert.equal(configuration(typed).timeout, 5000)
  assert.equal(patch(typed).timeout, 5000)
  const remote = draft({ type: "http", url: "https://example.test/mcp", timeout: 5000 })
  assert.equal(configuration(remote).timeout, 5000)
  assert.equal(patch(remote).timeout, 5000)
})

test("a kept string timeout still saves as the number", () => {
  const typed = draft({ timeout: "5000" })
  assert.equal(configuration(typed).timeout, 5000)
  assert.equal(patch(typed).timeout, 5000)
  const blank = draft({ timeout: "" })
  assert.equal("timeout" in configuration(blank), false)
  assert.equal(patch(blank).timeout, null)
})

/// #952: the daemon expands a reference, so the form accepts an endpoint
/// template and a malformed literal endpoint still stops the save.
test("an endpoint template is a valid draft and a malformed literal URL is not", async () => {
  const saves = []
  globalThis.window = { confirm: () => true }
  globalThis.settingsConnection = reactive({
    phase: "connected",
    supports: () => true,
    client: { call: async (method, params, read) => {
      if (method === "mcp.catalog") return read(catalog(undefined))
      assert.equal(method, "mcp.save")
      saves.push(JSON.parse(JSON.stringify(params)))
      return read({})
    } },
  })
  const root = node("root")
  const app = renderer.createApp(McpSettings)
  app.mount(root)
  try {
    await flush()
    await button(root, "Add server").props.onClick()
    await flush()
    enter(field(root, "Server ID"), "template")
    enter(field(root, "Transport"), "http")
    await flush()
    const url = field(root, "URL")
    assert.equal(url.props.type, "text")
    assert.equal(url.props.inputmode, "url")
    enter(url, "${MCP_URL}/mcp")
    await flush()
    await submit(root)
    await flush()
    assert.equal(saves.length, 1)
    assert.equal(saves[0].configuration.type, "http")
    assert.equal(saves[0].configuration.url, "${MCP_URL}/mcp")

    await button(root, "Add server").props.onClick()
    await flush()
    enter(field(root, "Server ID"), "broken")
    enter(field(root, "Transport"), "http")
    await flush()
    enter(field(root, "URL"), "not a url")
    await flush()
    await submit(root)
    await flush()
    assert.equal(saves.length, 1, "the malformed literal URL never reaches the daemon")
    assert.match(text(root), /The endpoint must be an absolute URL/)
  } finally {
    app.unmount()
    delete globalThis.settingsConnection
    delete globalThis.window
  }
})
