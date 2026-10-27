<script setup lang="ts">
import { computed } from "vue";
import { textScale } from "../stores/ui";
import { wfIcons, type WfIconName } from "../lib/wficons";

const props = withDefaults(defineProps<{ name: WfIconName; size?: number }>(), { size: 20 });
const pixels = computed(() => Math.round(props.size * textScale.value));
</script>

<template>
  <span
    class="wf-icon"
    :style="{ '--wf-icon': `url(${wfIcons[name]})`, '--wf-size': `${pixels}px` }"
    aria-hidden="true"
  ></span>
</template>
