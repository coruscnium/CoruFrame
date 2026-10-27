<script setup lang="ts">
import { computed, watch } from "vue";
import { useRoute } from "vue-router";
import Icon from "./Icon.vue";
import ItemPanel from "./ItemPanel.vue";
import MarketPanel from "./MarketPanel.vue";
import TimersPanel from "./TimersPanel.vue";
import { app } from "../stores/app";

const route = useRoute();
const isCollection = computed(() => route.path === "/mastery");

// The Item tab belongs to the collection; keep it from following the user
// into the inventory, rivens and the other workspaces.
watch(
  () => route.path,
  (path) => {
    if (path !== "/mastery" && app.railTab === "item") app.railTab = "timers";
  },
  { immediate: true },
);
</script>

<template>
  <aside class="right-rail" :class="{ open: app.railOpen }">
    <div class="rail-tabs" :class="{ 'has-item': isCollection }" role="tablist" aria-label="Live panels">
      <button
        type="button"
        role="tab"
        class="rail-tab"
        :class="{ active: app.railTab === 'timers' }"
        :aria-selected="app.railTab === 'timers'"
        @click="app.railTab = 'timers'"
      >
        <Icon name="clock" :size="15" />
        Timers
      </button>
      <button
        v-if="isCollection"
        type="button"
        role="tab"
        class="rail-tab"
        :class="{ active: app.railTab === 'item' }"
        :aria-selected="app.railTab === 'item'"
        @click="app.railTab = 'item'"
      >
        <Icon name="inventory" :size="15" />
        Item
      </button>
      <button
        type="button"
        role="tab"
        class="rail-tab"
        :class="{ active: app.railTab === 'market' }"
        :aria-selected="app.railTab === 'market'"
        @click="app.railTab = 'market'"
      >
        <Icon name="shop" :size="15" />
        Market
      </button>
      <button
        class="rail-close"
        type="button"
        aria-label="Close live panels"
        @click="app.railOpen = false"
      >
        <Icon name="x" :size="15" />
      </button>
    </div>
    <div class="rail-scroll">
      <TimersPanel v-if="app.railTab === 'timers'" />
      <ItemPanel v-else-if="isCollection && app.railTab === 'item'" />
      <MarketPanel v-else-if="app.railTab === 'market'" />
      <TimersPanel v-else />
    </div>
  </aside>
</template>
