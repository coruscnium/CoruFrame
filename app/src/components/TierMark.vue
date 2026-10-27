<script setup lang="ts">
import { computed } from "vue";
import { relicImage } from "../lib/equipment";

const props = defineProps<{ tier: string; refinement?: string | null }>();

const label = computed(() => {
  const tier = props.tier.charAt(0).toUpperCase() + props.tier.slice(1);
  return props.refinement ? `${tier} ${props.refinement}` : tier;
});
const source = computed(() => relicImage(props.tier, props.refinement ?? "Intact"));
const letter = computed(() => props.tier.charAt(0).toUpperCase());
</script>

<template>
  <span
    class="tier-mark"
    :class="`tier-${tier.toLowerCase()}`"
    role="img"
    :aria-label="label"
    :title="label"
  >
    <img
      v-if="source"
      class="tier-thumb"
      :src="source"
      alt=""
      loading="lazy"
      decoding="async"
      draggable="false"
    />
    <span v-else>{{ letter }}</span>
  </span>
</template>
