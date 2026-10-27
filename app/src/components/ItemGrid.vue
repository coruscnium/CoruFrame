<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import Icon from "./Icon.vue";
import ItemCard from "./ItemCard.vue";
import type { CatalogCard } from "../stores/catalog";

const props = defineProps<{
  cards: CatalogCard[];
  ownedNames: Set<string>;
  masteredNames: Set<string>;
  componentCounts?: Record<string, number>;
}>();

const OVERSCAN_ROWS = 2;

const root = ref<HTMLElement | null>(null);
const grid = ref<HTMLElement | null>(null);
const scroller = ref<HTMLElement | null>(null);
const cols = ref(3);
const cardHeight = ref(210);
const gap = ref(15);
const scrollTop = ref(0);
const viewportHeight = ref(900);
const gridTop = ref(0);
let frame = 0;
let observer: ResizeObserver | null = null;

function measure() {
  const gridElement = grid.value;
  if (!gridElement) return;
  const styles = getComputedStyle(gridElement);
  // Read the real tracks so a future 2- or 4-column breakpoint needs no JS change.
  cols.value = Math.max(
    1,
    styles.gridTemplateColumns.split(" ").filter(Boolean).length,
  );
  gap.value = Number.parseFloat(styles.rowGap) || 15;
  // `--card-h` is a calc() over the text scale, so parseFloat cannot read it.
  // A rendered card already has the resolved height; with no card in the window
  // the base size times the scale is the same value.
  const scale = Number.parseFloat(styles.getPropertyValue("--font-scale")) || 1;
  cardHeight.value =
    gridElement.querySelector<HTMLElement>(".item-card")?.offsetHeight ||
    Math.round(210 * scale);
}

function update() {
  const element = scroller.value;
  const wrapper = root.value;
  if (!element || !wrapper) return;
  scrollTop.value = element.scrollTop;
  viewportHeight.value = element.clientHeight;
  // The grid sits below a scrolling header, so its own top matters.
  gridTop.value = Math.max(
    0,
    wrapper.getBoundingClientRect().top -
      element.getBoundingClientRect().top +
      element.scrollTop,
  );
}

function scheduleUpdate() {
  if (frame) return;
  frame = requestAnimationFrame(() => {
    frame = 0;
    update();
  });
}

function refresh() {
  nextTick(() => {
    measure();
    update();
  });
}

const rowHeight = computed(() => cardHeight.value + gap.value);
const totalRows = computed(() =>
  Math.ceil(props.cards.length / cols.value),
);
const totalHeight = computed(() =>
  Math.max(0, totalRows.value * rowHeight.value - gap.value),
);
const startRow = computed(() => {
  const local = Math.max(0, scrollTop.value - gridTop.value);
  return Math.max(0, Math.floor(local / rowHeight.value) - OVERSCAN_ROWS);
});
const endRow = computed(() => {
  const local = Math.max(0, scrollTop.value - gridTop.value);
  const last = Math.ceil((local + viewportHeight.value) / rowHeight.value) + OVERSCAN_ROWS;
  return Math.min(totalRows.value, last);
});
const visible = computed(() =>
  props.cards.slice(
    startRow.value * cols.value,
    endRow.value * cols.value,
  ),
);
const offset = computed(() => startRow.value * rowHeight.value);

// The wrapper only exists once cards arrive, so binding happens on the element,
// not only at mount.
function attach() {
  const previous = scroller.value;
  previous?.removeEventListener("scroll", scheduleUpdate);
  observer?.disconnect();
  observer = null;

  scroller.value = root.value?.closest<HTMLElement>(".workspace") ?? null;
  if (root.value) {
    observer = new ResizeObserver(scheduleUpdate);
    observer.observe(root.value);
    observer.observe(scroller.value ?? root.value);
  }
  scroller.value?.addEventListener("scroll", scheduleUpdate, { passive: true });
  refresh();
}

onMounted(() => {
  window.addEventListener("resize", scheduleUpdate);
  attach();
  void document.fonts?.ready.then(refresh);
});

onUnmounted(() => {
  cancelAnimationFrame(frame);
  scroller.value?.removeEventListener("scroll", scheduleUpdate);
  window.removeEventListener("resize", scheduleUpdate);
  observer?.disconnect();
});

watch(root, () => attach());

watch(
  () => props.cards,
  () => {
    // A shorter result set must clamp the window even without a scroll event.
    refresh();
  },
);
</script>

<template>
  <div
    v-if="cards.length"
    ref="root"
    class="item-grid-window"
    :style="{ height: `${totalHeight}px` }"
  >
    <div
      ref="grid"
      class="item-grid"
      :style="{ transform: `translateY(${offset}px)` }"
    >
      <ItemCard
        v-for="card in visible"
        :key="card.item.unique_name"
        :card="card"
        :owned="card.owned ?? ownedNames.has(card.item.unique_name)"
        :mastered="card.mastered ?? masteredNames.has(card.item.unique_name)"
        :component-counts="componentCounts"
      />
    </div>
  </div>
  <div v-else class="empty-state">
    <Icon name="search" :size="30" />
    <h3>No items match</h3>
    <p>Try another category, or clear the search and filters.</p>
  </div>
</template>
