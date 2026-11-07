<script setup lang="ts">
// Lacus brand mark: the app icon itself (desktop/icon.svg cropped to its body, in
// assets/lacus-icon.svg), so the window shows the picture the Dock does. `animated`: the
// icon breathes, the startup loading indicator. index.html draws the same icon before the
// bundle loads, and as its favicon; keep them in step.
import icon from "../assets/lacus-icon.svg"

withDefaults(defineProps<{ animated?: boolean }>(), { animated: false })
</script>

<template>
  <svg viewBox="0 0 824 824" aria-hidden="true" class="brand-mark" :class="{ 'is-animated': animated }">
    <image :href="icon" width="824" height="824" />
  </svg>
</template>

<style scoped>
.brand-mark.is-animated { animation: brand-mark-breathe 1.6s ease-in-out infinite; }
@keyframes brand-mark-breathe {
  0%, 100% { opacity: .6; transform: scale(.94); }
  50% { opacity: 1; transform: scale(1); }
}
@media (prefers-reduced-motion: reduce) {
  .brand-mark.is-animated { animation: none; }
}
</style>
