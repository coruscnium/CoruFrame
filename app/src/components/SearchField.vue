<script setup lang="ts">
import Icon from "./Icon.vue";

defineProps<{ placeholder: string; label?: string }>();
const model = defineModel<string>({ required: true });
</script>

<template>
  <label class="search-field">
    <Icon name="search" :size="16" />
    <input
      v-model="model"
      type="search"
      :placeholder="placeholder"
      :aria-label="label ?? placeholder"
      spellcheck="false"
      autocomplete="off"
      autocapitalize="off"
    />
  </label>
</template>
