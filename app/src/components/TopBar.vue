<script setup lang="ts">
import Icon from "./Icon.vue";
import WfIcon from "./WfIcon.vue";
import SearchField from "./SearchField.vue";
import RailToggle from "./RailToggle.vue";
import { computed, ref, watch } from "vue";
import { categories } from "../data/mock";
import { app, toggleLayout } from "../stores/app";
import { inventory } from "../stores/inventory";
import { categoryIcon } from "../lib/wficons";
import { useCompactTabs } from "../lib/useCompactTabs";

const tabs = ref<HTMLElement | null>(null);
const compact = useCompactTabs(tabs);
// Six on top, five below: each row spreads its own tabs evenly.
const categoryRows = computed(() => {
  const middle = Math.ceil(categories.length / 2);
  return [categories.slice(0, middle), categories.slice(middle)];
});

const masteryAvailable = computed(
  () => inventory.snapshot?.mastery_available === true,
);

function onTabsKeydown(event: KeyboardEvent) {
  if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
  const list = categories;
  const index = list.findIndex((category) => category.id === app.category);
  const step = event.key === "ArrowRight" ? 1 : -1;
  const next = (index + step + list.length) % list.length;
  event.preventDefault();
  app.category = list[next].id;
  requestAnimationFrame(() => {
    const tabs = document.querySelectorAll<HTMLElement>(".equipment-tab");
    tabs[next]?.focus();
    tabs[next]?.scrollIntoView({ block: "nearest", inline: "nearest" });
  });
}

watch(
  [() => app.category, masteryAvailable],
  () => {
    if (!categories.some((category) => category.id === app.category)) {
      app.category = "Warframes";
    }
    const allowed = new Set(["all", "owned", "missing", "vaulted"]);
    if (masteryAvailable.value) {
      allowed.add("unmastered");
      allowed.add("mastered");
    }
    if (!allowed.has(app.filter)) app.filter = "all";
  },
  { immediate: true },
);

// Each filter has its own natural order: name A-Z, or most progressed first.
watch(
  () => app.filter,
  (filter) => {
    app.sortAsc = filter !== "unmastered";
  },
  { immediate: true },
);
</script>

<template>
  <header class="topbar">
    <div class="topbar-actions">
      <SearchField v-model="app.search" placeholder="Search collection…" label="Search" />
      <label class="select-field">
        <Icon name="filter" :size="15" />
        <select v-model="app.filter" aria-label="Filter items">
          <option value="all">All items</option>
          <option value="owned">Owned</option>
          <option value="missing">Missing</option>
          <option v-if="masteryAvailable" value="unmastered">Unmastered</option>
          <option v-if="masteryAvailable" value="mastered">Mastered</option>
          <option value="vaulted">Vaulted</option>
        </select>
      </label>
      <button
        class="icon-button"
        type="button"
        :title="app.sortAsc ? 'Ascending' : 'Descending'"
        :aria-label="app.sortAsc ? 'Ascending' : 'Descending'"
        @click="app.sortAsc = !app.sortAsc"
      >
        <Icon :name="app.sortAsc ? 'arrowUp' : 'arrowDown'" :size="18" />
      </button>
      <button
        class="icon-button"
        type="button"
        :title="app.layout === 'grid' ? 'Switch to list view' : 'Switch to grid view'"
        :aria-label="app.layout === 'grid' ? 'Switch to list view' : 'Switch to grid view'"
        @click="toggleLayout"
      >
        <Icon :name="app.layout === 'grid' ? 'list' : 'grid'" :size="18" />
      </button>
      <RailToggle />
    </div>

    <div
      ref="tabs"
      class="equipment-tabs"
      :class="{ compact }"
      role="tablist"
      aria-label="Item categories"
      @keydown="onTabsKeydown"
    >
      <div
        v-for="(row, index) in categoryRows"
        :key="index"
        class="equipment-row"
      >
        <button
          v-for="category in row"
          :key="category.id"
          type="button"
          role="tab"
          class="equipment-tab"
          :class="{ active: app.category === category.id }"
          :aria-selected="app.category === category.id"
          :aria-label="category.label"
          :title="category.label"
          @click="app.category = category.id"
        >
          <WfIcon :name="categoryIcon(category.id)" :size="19" />
          <span class="tab-label">{{ category.label }}</span>
        </button>
      </div>
    </div>
  </header>
</template>
