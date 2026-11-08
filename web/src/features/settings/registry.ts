import type { Component } from "vue"
import { Activity, Archive, Box, Brain, Cog, Globe, Monitor, Moon, Palette, Plug, Settings2, ShieldCheck, SlidersHorizontal, Sun, UserCircle, Wrench } from "lucide-vue-next"
import { canChooseWorkspace as inDesktop } from "../../desktop"
import type { TranslationKey } from "../../i18n"
import { useAppearanceStore } from "../../stores/appearance"
import { useConnectionStore } from "../../stores/connection"
import { TRANSCRIPT_VIEWS, usePreferencesStore, type TranscriptView } from "../../stores/preferences"

/**
 * The settings pages and the browser's preferences as declarations, in the
 * shape of Claude Code's settings registry: the preference pages render their
 * rows from it, and the settings search finds pages and rows in it.
 */

/** Text in English and Chinese, as the settings pages write it. */
export type Bilingual = readonly [english: string, chinese: string]

export type SettingsPageId = "account" | "appearance" | "general" | "agent" | "providers" | "modality" | "mcp" | "plugins" | "skills" | "web" | "permissions" | "archived" | "diagnostics"

/** A settings page; `offline` ones work without the service, `keywords` help the search find it. */
export type SettingsPage = { id: SettingsPageId; icon: Component; keywords: Bilingual; offline?: boolean }

export const SETTINGS_PAGES: readonly SettingsPage[] = [
  { id: "account", icon: UserCircle, offline: true, keywords: ["sign in sign out plan usage credits", "登录 退出 套餐 用量 额度"] },
  { id: "appearance", icon: Palette, offline: true, keywords: ["theme dark light font size language transcript", "主题 深色 浅色 字号 语言 对话视图"] },
  { id: "general", icon: Cog, offline: true, keywords: ["keep awake sleep archive pull request", "休眠 归档 PR"] },
  { id: "agent", icon: Settings2, keywords: ["default model subagent member", "默认模型 子代理 成员"] },
  { id: "providers", icon: Brain, offline: true, keywords: ["model API key endpoint OpenAI Anthropic", "模型 密钥 接口"] },
  { id: "modality", icon: SlidersHorizontal, keywords: ["image audio vision speech", "图片 语音 视觉"] },
  { id: "mcp", icon: Wrench, keywords: ["server tools", "服务器 工具"] },
  { id: "plugins", icon: Plug, keywords: ["extension install", "扩展 安装"] },
  { id: "skills", icon: Box, keywords: ["skill instructions", "技能 指令"] },
  { id: "web", icon: Globe, keywords: ["search fetch browse", "搜索 抓取 联网"] },
  { id: "permissions", icon: ShieldCheck, keywords: ["allow deny rules tools sandbox", "允许 拒绝 规则 工具 沙箱"] },
  { id: "archived", icon: Archive, keywords: ["archive restore sessions", "归档 恢复 会话"] },
  { id: "diagnostics", icon: Activity, keywords: ["logs logging debug", "日志 调试"] },
]

export type SettingOption = { value: string | number; label: Bilingual | TranslationKey; icon?: Component }

type Base = {
  id: string
  page: "appearance" | "general"
  label: Bilingual
  description?: Bilingual
  keywords?: Bilingual
  /** Whether the row applies here; absent means always. */
  available?: () => boolean
}

/**
 * A preference row: a switch, a set of buttons (`cards` draws them large, with
 * their icons) or a drop-down, read and written through the store that keeps it.
 */
export type Setting =
  | Base & { control: "switch"; get: () => boolean; set: (value: boolean) => void }
  | Base & { control: "buttons" | "cards" | "select"; options: readonly SettingOption[]; get: () => string | number; set: (value: string | number) => void }

const FONT_SIZES = [12, 13, 14, 15, 16, 18] as const

export const SETTINGS: readonly Setting[] = [
  {
    id: "theme", page: "appearance", control: "cards", label: ["Theme", "主题"], keywords: ["dark light mode color", "深色 浅色 模式 配色"],
    options: [{ value: "light", label: ["Light", "浅色"], icon: Sun }, { value: "dark", label: ["Dark", "深色"], icon: Moon }, { value: "system", label: ["System", "跟随系统"], icon: Monitor }],
    get: () => useAppearanceStore().theme,
    set: (value) => { if (value === "light" || value === "dark" || value === "system") useAppearanceStore().theme = value },
  },
  {
    id: "font-size", page: "appearance", control: "buttons", label: ["UI font size", "界面字号"], description: ["Scales the entire interface, including controls and chat text.", "缩放整个界面，包括控件和聊天文本。"], keywords: ["zoom scale text", "缩放 文字 大小"],
    options: FONT_SIZES.map((size) => ({ value: size, label: [String(size), String(size)] as const })),
    get: () => useAppearanceStore().fontSize,
    set: (value) => { if (typeof value === "number") useAppearanceStore().fontSize = value },
  },
  {
    id: "transcript-view", page: "appearance", control: "buttons", label: ["Default transcript view", "默认对话视图"],
    description: ["Normal folds the agent's work into summaries, Thinking shows its thinking, Verbose opens every step. A session's ⋯ menu changes only that session.", "标准把 Agent 的工作折叠成摘要，思考展开思考过程，详细展开每一步。会话的“⋯”菜单只改该会话。"],
    keywords: ["normal thinking verbose", "标准 思考 详细"],
    options: TRANSCRIPT_VIEWS.map((view) => ({ value: view, label: `transcript.${view}` as const })),
    get: () => usePreferencesStore().transcriptView,
    set: (value) => { if (TRANSCRIPT_VIEWS.includes(value as TranscriptView)) usePreferencesStore().transcriptView = value as TranscriptView },
  },
  {
    id: "language", page: "appearance", control: "select", label: ["Language", "语言"], keywords: ["locale English Chinese", "界面语言 英文 中文"],
    options: [{ value: "", label: ["System default", "跟随系统"] }, { value: "en", label: ["English", "English"] }, { value: "zh-CN", label: ["简体中文", "简体中文"] }],
    get: () => useAppearanceStore().language,
    set: (value) => { if (value === "" || value === "en" || value === "zh-CN") void useAppearanceStore().selectLanguage(value) },
  },
  {
    id: "keep-awake", page: "general", control: "switch", label: ["Keep the computer awake while sessions run", "运行时防止休眠"],
    description: ["While a session runs, the computer does not sleep on its own, so long tasks can finish. The display can still turn off, and closing the lid still sleeps the computer.", "有会话运行时，电脑不会自动休眠，长任务可以跑完。显示器仍会关闭，合上笔记本盖子仍会休眠。"],
    keywords: ["sleep caffeinate", "睡眠 不休眠"],
    available: () => inDesktop,
    get: () => usePreferencesStore().keepAwake,
    set: (value) => { usePreferencesStore().keepAwake = value },
  },
  {
    id: "auto-archive", page: "general", control: "switch", label: ["Archive sessions when their pull request merges or closes", "PR 合并或关闭后自动归档"],
    description: ["Archives the sessions on a branch once its open pull request merges or closes; after a merge, a worktree Lacus created is removed and its branch kept. Sessions with uncommitted changes stay.", "分支上打开的 PR 合并或关闭后，归档在该分支上工作的会话；合并后还会移除 Lacus 创建的工作树，分支保留。有未提交改动的会话不会归档。"],
    keywords: ["pull request merge worktree", "拉取请求 合并 工作树"],
    available: () => ["git.pr", "git.status", "session.archive"].every((method) => useConnectionStore().supports(method)),
    get: () => usePreferencesStore().autoArchive,
    set: (value) => { usePreferencesStore().autoArchive = value },
  },
]

/** A page or row the settings search found. */
export type SettingsHit = { page: SettingsPageId; setting?: Setting }

/**
 * The rows, then the pages, whose words hold every word of `query` in either
 * language, each in page order. `pageNames` gives each page's names; rows that
 * do not apply here are left out.
 */
export function searchSettings(query: string, pageNames: (page: SettingsPageId) => string): SettingsHit[] {
  const words = query.toLocaleLowerCase().split(/\s+/).filter((word) => word !== "")
  if (words.length === 0) return []
  const matches = (...texts: (Bilingual | string | undefined)[]): boolean => {
    const haystack = texts.flat().filter((text) => text !== undefined).join(" ").toLocaleLowerCase()
    return words.every((word) => haystack.includes(word))
  }
  const rows = SETTINGS_PAGES.flatMap((page) => SETTINGS
    .filter((setting) => setting.page === page.id && (setting.available?.() ?? true) && matches(setting.label, setting.description, setting.keywords))
    .map((setting) => ({ page: page.id, setting })))
  return [...rows, ...SETTINGS_PAGES.filter((page) => matches(pageNames(page.id), page.keywords)).map((page) => ({ page: page.id }))]
}
