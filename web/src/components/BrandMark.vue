<script setup lang="ts">
// 野塘漫水 / Lacus brand mark: the app icon's lily pad, its drop of dew cut out. Colored via
// currentColor. `animated`: ripples spread from the pad, the startup loading indicator.
// index.html draws the same mark before the bundle loads, and its favicon; keep them in step.
withDefaults(defineProps<{ animated?: boolean }>(), { animated: false })

const PAD = "M30 35L45.43 53.38A24 24 0 1 0 39.76 56.92ZM33.1 25.9a5.5 5.5 0 1 0 11 0a5.5 5.5 0 1 0-11 0Z"
</script>

<template>
  <svg viewBox="0 0 64 64" aria-hidden="true" class="brand-mark" :class="{ 'is-animated': animated }">
    <template v-if="animated">
      <circle class="brand-ripple" cx="30" cy="35" r="26" />
      <circle class="brand-ripple" cx="30" cy="35" r="26" style="animation-delay: -0.9s" />
    </template>
    <path fill="currentColor" fill-rule="evenodd" :d="PAD" />
  </svg>
</template>

<style scoped>
.brand-mark { overflow: visible; }
.brand-ripple { fill: none; stroke: currentColor; stroke-width: 2; opacity: 0; transform-origin: 30px 35px; animation: brand-mark-ripple 1.8s ease-out infinite; }
@keyframes brand-mark-ripple {
  0% { opacity: .55; transform: scale(1); }
  100% { opacity: 0; transform: scale(1.32); }
}
@media (prefers-reduced-motion: reduce) {
  .brand-ripple { display: none; }
}
</style>
