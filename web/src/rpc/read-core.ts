/** The shared checks of the wire readers: each names where a value broke its contract. */
import type { ModelId, PermissionPolicy, PermissionPreset } from "./protocol"

export class ProtocolError extends Error {
  override name = "ProtocolError"
}

export type Json = Record<string, unknown>

export function fail(at: string, expected: string): never {
  throw new ProtocolError(`Invalid ${at}: expected ${expected}`)
}

export function obj(value: unknown, at: string): Json {
  if (value === null || typeof value !== "object" || Array.isArray(value)) fail(at, "an object")
  return value as Json
}

export function arr(value: unknown, at: string): unknown[] {
  if (!Array.isArray(value)) fail(at, "an array")
  return value
}

export function str(source: Json, key: string, at: string): string {
  const value = source[key]
  if (typeof value !== "string") fail(`${at}.${key}`, "a string")
  return value
}

export function optStr(source: Json, key: string, at: string): string | undefined {
  return source[key] === undefined || source[key] === null ? undefined : str(source, key, at)
}

export function num(source: Json, key: string, at: string): number {
  const value = source[key]
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) fail(`${at}.${key}`, "a nonnegative safe integer")
  return value
}

/** A nonnegative finite number; token counts are floats on the wire. */
export function finite(source: Json, key: string, at: string): number {
  const value = source[key]
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) fail(`${at}.${key}`, "a nonnegative number")
  return value
}

export function bool(source: Json, key: string, at: string): boolean {
  const value = source[key]
  if (typeof value !== "boolean") fail(`${at}.${key}`, "a boolean")
  return value
}

export function modelId(value: unknown, at: string): ModelId {
  const source = obj(value, at)
  return { providerId: str(source, "providerId", at), modelId: str(source, "modelId", at) }
}

/** A permission preset as the server names it; anything else is a protocol error. */
function readPreset(value: unknown, at: string): PermissionPreset {
  if (value === "read-only" || value === "ask" || value === "workspace-write" || value === "full-access") return value
  return fail(at, "a permission preset")
}

export function readPermissionPolicy(value: unknown, at: string): PermissionPolicy {
  const policy = obj(value, at)
  const list = (key: string) => arr(policy[key], `${at}.${key}`).map((item, index) => {
    if (typeof item !== "string") fail(`${at}.${key}[${index}]`, "a string")
    return item
  })
  return {
    preset: readPreset(policy.preset, `${at}.preset`),
    allowedTools: list("allowedTools"),
    disallowedTools: list("disallowedTools"),
  }
}

/** `session.permission.get`: the policy of `main` and the version a change must expect. */
