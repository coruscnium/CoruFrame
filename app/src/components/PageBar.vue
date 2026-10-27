<script setup lang="ts">
import RailToggle from "./RailToggle.vue";
</script>

<template>
  <header class="topbar">
    <div class="topbar-actions">
      <slot name="actions" />
      <RailToggle />
    </div>
    <div v-if="$slots.tabs" class="topbar-tabs">
      <slot name="tabs" />
    </div>
  </header>
</template>
