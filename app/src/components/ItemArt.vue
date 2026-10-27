<script setup lang="ts">
import { computed, ref, watch } from "vue";
import Icon from "./Icon.vue";
import { kindIcon } from "../lib/equipment";

defineOptions({ inheritAttrs: false });

const props = withDefaults(
  defineProps<{
    image?: string | null;
    marketIcon?: string | null;
    kind?: string;
    iconSize?: number;
    alt?: string;
    marketFirst?: boolean;
  }>(),
  { image: null, marketIcon: null, kind: "misc", iconSize: 20, alt: "", marketFirst: false },
);

const sources = computed(() => {
  // The item DB ships a generic placeholder for mods and arcanes; the market
  // thumb is the real art when it exists.
  const marketFirst =
    props.marketFirst || ["mod", "upgrade", "arcane"].includes(props.kind ?? "");
  const chain = marketFirst
    ? [props.marketIcon, props.image]
    : [props.image, props.marketIcon];
  return chain.filter((value): value is string => Boolean(value));
});
const index = ref(0);
const source = computed(() => sources.value[index.value] ?? null);

watch(sources, () => {
  index.value = 0;
});
</script>

<template>
  <img
    v-if="source"
    v-bind="$attrs"
    :src="source"
    :alt="alt"
    loading="lazy"
    decoding="async"
    draggable="false"
    referrerpolicy="no-referrer"
    @error="index += 1"
  />
  <Icon
    v-else
    :name="kindIcon(kind)"
    :size="iconSize"
    :class="[$attrs.class, 'item-art-kind']"
  />
</template>
