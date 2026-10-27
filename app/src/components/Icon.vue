<script setup lang="ts">
import { computed } from "vue";
import { filledIcons, icons, type IconName } from "../lib/icons";
import { textScale } from "../stores/ui";

const props = withDefaults(
  defineProps<{ name: IconName; size?: number; stroke?: number }>(),
  { size: 18, stroke: 1.8 },
);

const paths = computed(() => icons[props.name] ?? []);
const filled = computed(() => filledIcons.has(props.name));
const pixels = computed(() => Math.round(props.size * textScale.value));
</script>

<template>
  <svg
    :width="pixels"
    :height="pixels"
    viewBox="0 0 24 24"
    :fill="filled ? 'currentColor' : 'none'"
    stroke="currentColor"
    :stroke-width="stroke"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
    focusable="false"
  >
    <path v-for="(d, index) in paths" :key="index" :d="d" />
  </svg>
</template>
