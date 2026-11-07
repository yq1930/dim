/** Readers for Provider replies: lists, catalogs and the global model selection. */
import type { ModelId, ProviderCatalog, ProviderSummary } from "./protocol"
import { fail, obj, arr, str, optStr, bool, modelId, type Json } from "./read-core"

export function readProviderList(value: unknown): ProviderSummary[] {
  const source = obj(value, "provider.list")
  return arr(source.providers, "provider.list.providers").map((item, index) => {
    const at = `provider.list.providers[${index}]`
    const provider = obj(item, at)
    return {
      providerId: str(provider, "providerId", at),
      displayName: str(provider, "displayName", at),
      enabled: bool(provider, "enabled", at),
      credentialConfigured: bool(provider, "credentialConfigured", at),
    }
  })
}

export function readAccountProviders(value: unknown): Pick<ProviderSummary, "providerId" | "displayName">[] {
  return arr(value, "account.providers").map((item, index) => {
    const at = `account.providers[${index}]`
    const provider = obj(item, at)
    return { providerId: str(provider, "providerId", at), displayName: str(provider, "displayName", at) }
  })
}

/** `provider.catalog`: `{catalog: null}` when the provider has no catalog. */
/**
 * The reasoning effort values a catalog model accepts. Undefined unless the
 * model advertises reasoning preferences, as `/effort` requires (`configure.rs`).
 */
function readReasoningEfforts(model: Json, at: string): string[] | undefined {
  const metadata = model.metadata === undefined || model.metadata === null ? undefined : obj(model.metadata, `${at}.metadata`)
  const reasoning = metadata?.reasoning === undefined || metadata.reasoning === null ? undefined : obj(metadata.reasoning, `${at}.metadata.reasoning`)
  if (reasoning === undefined || reasoning.supported !== true) return undefined
  const capabilities = obj(model.capabilities, `${at}.capabilities`)
  if (capabilities.reasoning !== true) return undefined
  const efforts = reasoning.effortOptions === undefined || reasoning.effortOptions === null
    ? []
    : arr(reasoning.effortOptions, `${at}.metadata.reasoning.effortOptions`).map((effort, index) => {
      if (typeof effort !== "string") fail(`${at}.metadata.reasoning.effortOptions[${index}]`, "a string")
      return effort
    })
  if (reasoning.off !== undefined && reasoning.off !== null && !efforts.includes("none")) efforts.push("none")
  return efforts
}

export function readProviderCatalog(value: unknown): ProviderCatalog | undefined {
  const source = obj(value, "provider.catalog")
  if (source.catalog === null) return undefined
  const catalog = obj(source.catalog, "provider.catalog.catalog")
  return {
    models: arr(catalog.models, "provider.catalog.models").map((item, index) => {
      const at = `provider.catalog.models[${index}]`
      const model = obj(item, at)
      const unavailableReason = optStr(model, "unavailableReason", at)
      const reasoningEfforts = readReasoningEfforts(model, at)
      return {
        id: str(model, "id", at),
        name: str(model, "name", at),
        ...(unavailableReason === undefined ? {} : { unavailableReason }),
        ...(reasoningEfforts === undefined ? {} : { reasoningEfforts }),
      }
    }),
    disabledModelIds: arr(catalog.disabledModelIds, "provider.catalog.disabledModelIds").map((id, index) => {
      if (typeof id !== "string") fail(`provider.catalog.disabledModelIds[${index}]`, "a string")
      return id
    }),
  }
}

/**
 * `provider.selection`: the default model for a directory. The selection
 * object serializes in snake_case (`ProviderSelection` has no rename).
 */
export function readProviderSelection(value: unknown): ModelId | undefined {
  const source = obj(value, "provider.selection")
  if (source.selection === null) return undefined
  const selection = obj(source.selection, "provider.selection.selection")
  return { providerId: str(selection, "provider_id", "provider.selection"), modelId: str(selection, "model_id", "provider.selection") }
}
