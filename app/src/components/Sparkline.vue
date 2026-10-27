<script setup lang="ts">
import { computed } from "vue";

const props = withDefaults(
  defineProps<{ values: number[]; width?: number; height?: number; tone?: "up" | "down" | "flat" }>(),
  { width: 76, height: 24, tone: "flat" },
);

const points = computed(() => {
  const values = props.values.length > 1 ? props.values : [props.values[0] ?? 0, props.values[0] ?? 0];
  const min = Math.min(...values);
  const max = Math.max(...values);
  const span = max - min || 1;
  return values
    .map((value, index) => {
      const x = (index / (values.length - 1)) * props.width;
      const y = props.height - ((value - min) / span) * (props.height - 3) - 2;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
});
</script>

<template>
  <svg
    class="sparkline"
    :class="`tone-${tone}`"
    :width="width"
    :height="height"
    :viewBox="`0 0 ${width} ${height}`"
    aria-hidden="true"
  >
    <polyline :points="points" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" />
  </svg>
</template>
