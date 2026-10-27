<script setup lang="ts">
import { computed } from "vue";
import platinum from "../assets/wf/Platinum.png";
import ducats from "../assets/wf/Ducat.png";
import { textScale } from "../stores/ui";

const props = withDefaults(
  defineProps<{ name: "platinum" | "ducats"; size?: number }>(),
  { size: 14 },
);
const pixels = computed(() => Math.round(props.size * textScale.value));
const source = computed(() => (props.name === "platinum" ? platinum : ducats));
</script>

<template>
  <img
    class="currency-icon"
    :src="source"
    :style="{ width: `${pixels}px`, height: `${pixels}px` }"
    alt=""
    aria-hidden="true"
    draggable="false"
  />
</template>
