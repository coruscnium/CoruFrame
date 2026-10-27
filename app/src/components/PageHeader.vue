<script setup lang="ts">
import Icon from "./Icon.vue";
import WfIcon from "./WfIcon.vue";
import RailToggle from "./RailToggle.vue";
import type { IconName } from "../lib/icons";
import type { WfIconName } from "../lib/wficons";

withDefaults(
  defineProps<{
    title: string;
    description?: string;
    icon?: WfIconName;
    svgIcon?: IconName;
    railToggle?: boolean;
  }>(),
  { description: "", icon: undefined, svgIcon: undefined, railToggle: false },
);
</script>

<template>
  <header class="page-head">
    <div class="page-head-main">
      <span class="page-icon">
        <WfIcon v-if="icon" :name="icon" :size="20" />
        <Icon v-else-if="svgIcon" :name="svgIcon" :size="19" />
      </span>
      <h1>{{ title }}</h1>
      <div v-if="description || $slots.description" class="page-meta">
        <slot name="description">{{ description }}</slot>
      </div>
    </div>
    <div class="page-actions"><slot name="actions" /><RailToggle v-if="railToggle" /></div>
  </header>
</template>
