<script setup lang="ts">
import Icon from "./Icon.vue";
import { app } from "../stores/app";
</script>

<template>
  <button
    class="rail-toggle"
    :class="{ active: app.railOpen }"
    type="button"
    aria-label="Live panels"
    :aria-expanded="app.railOpen"
    @click="app.railOpen = !app.railOpen"
  >
    <Icon name="panelRight" :size="18" />
  </button>
</template>
