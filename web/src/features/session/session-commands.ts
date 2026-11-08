import { computed, type ComputedRef, type Ref } from "vue"
import { useRouter } from "vue-router"
import { useI18n } from "vue-i18n"
import { copyText } from "../../clipboard"
import { goalCall, goalObjective, parseGoalRequest, type Goal, type GoalStatus } from "../../goal"
import { MAIN_BRANCH, type ContextUsage, type SessionSummary } from "../../rpc/protocol"
import { useChatStore, type SessionView } from "../../stores/chat"
import { useConnectionStore } from "../../stores/connection"
import { useModelsStore } from "../../stores/models"
import { useSessionsStore } from "../../stores/sessions"
import { useToastStore } from "../../stores/toast"
import { usePanesStore } from "../../stores/panes"
import { formatTokens } from "./composer/usage-format"

/**
 * The Session page's own composer commands — /compact, /usage, /fork, /goal,
 * /effort, /review, /status, /copy and /diff — each offered only where the
 * Session serves what it needs, with results and failures reported as notices.
 * `confirm` asks before a fork; `contextUsage` is the composer's meter, which
 * /compact refreshes.
 */
export function useSessionCommands(options: {
  sessionId: () => string
  view: ComputedRef<SessionView | undefined>
  summary: ComputedRef<SessionSummary | undefined>
  contextUsage: Ref<ContextUsage | undefined>
  confirm: (message: string, options: { confirmLabel: string }) => Promise<boolean | undefined>
}) {
  const { sessionId, view, summary, contextUsage, confirm } = options
  const { t } = useI18n()
  const chat = useChatStore()
  const connection = useConnectionStore()
  const models = useModelsStore()
  const sessions = useSessionsStore()
  const toast = useToastStore()
  const panes = usePanesStore()
  const router = useRouter()

  /** The reasoning effort values the main line's model accepts; undefined until the models are read or when it has none. */
  const effortChoices = computed(() => {
    const model = view.value?.model
    if (model === undefined) return undefined
    const entry = models.providers?.find((provider) => provider.providerId === model.providerId)?.models.find((candidate) => candidate.id === model.modelId)
    return entry?.reasoningEfforts
  })
  const commands = computed(() => {
    const session = view.value
    if (session?.status !== "ready") return undefined
    return {
      compact: session.methods.includes("session.context.compact") ? compact : undefined,
      usage: session.methods.includes("session.context.usage") ? usage : undefined,
      fork: connection.supports("session.fork") && session.lastTurnEnd !== undefined ? fork : undefined,
      goal: session.methods.includes("session.goal.set") ? goal : undefined,
      effort: effortChoices.value !== undefined && effortChoices.value.length > 0 ? effort : undefined,
      review,
      status,
      copy: copyLastReply,
      diff: () => panes.show(sessionId(), "changes"),
    }
  })

  function failure(cause: unknown): void {
    toast.push({ tone: "danger", title: t("command.failed"), description: cause instanceof Error ? cause.message : String(cause) })
  }

  async function compact(): Promise<void> {
    try {
      const result = await chat.compact(sessionId())
      if (result.status === "completed") {
        toast.push({ tone: "success", title: t("command.compacted") })
        contextUsage.value = await chat.usage(sessionId())
      }
      else toast.push({ tone: "info", title: t("command.compactSkipped"), description: result.reason })
    } catch (cause) {
      failure(cause)
    }
  }

  /** `/usage`'s one line, as the TUI words it. */
  function usageLine(reading: ContextUsage): string {
    const input = Math.round(reading.estimatedInputTokens)
    const window = Math.round(reading.contextWindow)
    return t("command.usage", {
      input: input.toLocaleString(),
      window: window.toLocaleString(),
      percent: window > 0 ? Math.floor((input * 100) / window) : 0,
      basis: reading.baselineSource === "provider_baseline" ? t("command.basisProvider") : t("command.basisEstimated"),
      output: Math.round(reading.outputTokens).toLocaleString(),
      threshold: Math.round(reading.thresholdTokens).toLocaleString(),
    })
  }

  async function usage(): Promise<void> {
    try {
      toast.push({ tone: "info", title: t("command.usageTitle"), description: usageLine(await chat.usage(sessionId())) })
    } catch (cause) {
      failure(cause)
    }
  }

  function statusWords(status: GoalStatus): string {
    switch (status.kind) {
      case "active": return t("goal.status.active")
      case "paused": return t("goal.status.paused", { reason: status.reason.replaceAll("_", " ") })
      case "completed": return t("goal.status.completed")
      case "blocked": return t("goal.status.blocked")
      case "none": return t("goal.status.none")
    }
  }

  /** `/goal` with no words: the goal in force on main. */
  function showGoal(current: Goal | undefined): void {
    if (current === undefined) {
      toast.push({ tone: "info", title: t("goal.none") })
      return
    }
    toast.push({
      tone: "info",
      title: `${t("goal.line", { status: statusWords(current.status) })}${current.evaluator ? ` · ${t("goal.evaluator")}` : ""}`,
      description: current.objective,
    })
  }

  async function goal(words: string): Promise<void> {
    const request = parseGoalRequest(words)
    if (request === undefined) {
      toast.push({ tone: "info", title: t("goal.usage") })
      return
    }
    const current = view.value?.goal
    const call = goalCall(request, current)
    if (call === undefined) {
      showGoal(current)
      return
    }
    try {
      await chat.writeGoal(sessionId(), call, goalObjective(request))
      toast.push({ tone: "success", title: t("goal.sent", { action: t(`goal.action.${call}`) }) })
    } catch (cause) {
      failure(cause)
    }
  }

  /** `/effort` with no words lists the choices; `/effort VALUE` sets the main line's reasoning effort. */
  async function effort(words: string): Promise<void> {
    const choices = effortChoices.value
    const value = words.trim()
    if (choices === undefined) {
      toast.push({ tone: "info", title: t("command.effortUnavailable") })
      return
    }
    if (value === "") {
      toast.push({ tone: "info", title: t("command.effortChoices", { choices: choices.join(", ") }) })
      return
    }
    if (/\s/.test(value)) {
      toast.push({ tone: "info", title: t("command.effortUsage") })
      return
    }
    if (!choices.includes(value)) {
      toast.push({ tone: "danger", title: t("command.effortUnknown") })
      return
    }
    try {
      const outcome = await chat.setEffort(sessionId(), value)
      toast.push({ tone: "success", title: t("command.effortSet", { outcome, value }) })
    } catch (cause) {
      failure(cause)
    }
  }

  /** `/review [focus]`: asks the main line to review the workspace's uncommitted changes, as Codex's /review does. */
  async function review(focus: string): Promise<void> {
    const prompt = focus === "" ? t("review.prompt") : `${t("review.prompt")}\n\n${t("review.focus", { focus })}`
    try {
      await chat.submit(sessionId(), MAIN_BRANCH, prompt)
    } catch (cause) {
      failure(cause)
    }
  }

  /** `/status`: the main line's model, mode, context and workspace in one notice. */
  function status(): void {
    const session = view.value
    if (session === undefined) return
    const model = session.model
    const provider = models.providers?.find((candidate) => candidate.providerId === model?.providerId)
    const modelName = provider?.models.find((candidate) => candidate.id === model?.modelId)?.name ?? model?.modelId
    const mode = session.planning ? t("composer.mode.plan") : session.permission ? t(`composer.mode.${session.permission.preset}`) : undefined
    const reading = contextUsage.value
    const lines = [
      t("status.model", { model: [provider?.name, modelName].filter(Boolean).join(" / ") || "—", effort: session.reasoningEffort ?? t("status.defaultEffort") }),
      ...(mode === undefined ? [] : [t("status.mode", { mode })]),
      ...(reading === undefined || reading.contextWindow <= 0 ? [] : [t("status.context", { input: formatTokens(reading.estimatedInputTokens), window: formatTokens(reading.contextWindow), percent: Math.floor((reading.estimatedInputTokens * 100) / reading.contextWindow) })]),
      ...(summary.value ? [t("status.workspace", { cwd: summary.value.cwd })] : []),
      t("status.session", { id: sessionId().slice(0, 8), version: connection.hello?.serverVersion ?? "" }),
    ]
    toast.push({ tone: "info", title: t("status.title"), description: lines.join("\n") })
  }

  /** `/copy`: the main line's last reply, as its Markdown. */
  async function copyLastReply(): Promise<void> {
    const reply = [...(view.value?.rows ?? [])].reverse().find((row) => row.kind === "assistant" && row.text.trim() !== "")
    if (reply?.kind !== "assistant") {
      toast.push({ tone: "info", title: t("copy.none") })
      return
    }
    try {
      await copyText(reply.text)
      toast.push({ tone: "success", title: t("copy.done") })
    } catch (cause) {
      failure(cause)
    }
  }

  async function fork(): Promise<void> {
    const at = view.value?.lastTurnEnd
    if (at === undefined) return
    if (!await confirm(t("fork.confirm"), { confirmLabel: t("fork.confirmLabel") })) return
    try {
      const forked = await chat.fork(sessionId(), at)
      void sessions.refresh()
      await router.push({ name: "chat-session", params: { sessionId: forked } })
    } catch (cause) {
      failure(cause)
    }
  }

  return { commands, failure }
}
