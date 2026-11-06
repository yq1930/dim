<script setup lang="ts">
import { ref } from "vue"
import { useI18n } from "vue-i18n"
import { ListChecks } from "lucide-vue-next"
import type { PendingPlan } from "../../rpc/protocol"
import { RpcError } from "../../rpc/client"
import { useChatStore } from "../../stores/chat"
import ChatMarkdown from "./ChatMarkdown.vue"

/**
 * The `exit_plan_mode` review: it takes the composer's place and shows the
 * submitted plan. Approving ends plan mode so the agent carries the plan out;
 * keeping on planning sends it back with the typed feedback. The card closes
 * when the call's result reaches the timeline.
 */
const props = defineProps<{ sessionId: string; review: PendingPlan }>()
const { t } = useI18n()
const chat = useChatStore()
const feedback = ref("")
const busy = ref(false)
const sent = ref(false)
const error = ref("")

async function decide(approved: boolean): Promise<void> {
  if (busy.value || sent.value) return
  busy.value = true
  error.value = ""
  try {
    await chat.reviewPlan(props.sessionId, props.review, approved, feedback.value)
    sent.value = true
  } catch (cause) {
    error.value = cause instanceof RpcError && cause.appError === "plan_not_pending" ? t("plan.notPending") : cause instanceof Error && cause.message ? cause.message : t("plan.failed")
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <article class="prompt-card" data-inline-interaction-prompt>
    <header class="prompt-header">
      <span class="prompt-icon" aria-hidden="true"><ListChecks :size="12" :stroke-width="2.25" /></span>
      <h2 class="prompt-title">{{ t("plan.title") }}</h2>
      <span class="ui-badge" data-tone="warning">{{ sent ? t("plan.sent") : t("plan.awaiting") }}</span>
    </header>
    <div class="plan-body">
      <ChatMarkdown :content="review.plan" :final="true" :message-id="`plan-${review.requestId}-${review.callId}`" />
    </div>
    <textarea v-model="feedback" class="ui-field plan-feedback" rows="2" :placeholder="t('plan.feedbackPlaceholder')" :aria-label="t('plan.feedback')" :disabled="busy || sent" />
    <p v-if="error" class="prompt-error" role="alert">{{ error }}</p>
    <div class="prompt-buttons">
      <button type="button" class="ui-button" data-size="md" data-variant="ghost" :disabled="busy || sent" @click="decide(false)">{{ t("plan.keepPlanning") }}</button>
      <button type="button" class="ui-button" data-size="md" data-variant="primary" :disabled="busy || sent" @click="decide(true)">{{ t("plan.approve") }}</button>
    </div>
  </article>
</template>

<style scoped>
.plan-body { max-height: min(50vh, 420px); margin-top: var(--space-3); overflow-y: auto; overscroll-behavior: contain; }
.plan-feedback { width: 100%; margin-top: var(--space-3); resize: vertical; }
</style>
