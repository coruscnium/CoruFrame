<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, shallowReactive, ref, watch } from "vue";
import { RouterLink } from "vue-router";
import CurrencyIcon from "../components/CurrencyIcon.vue";
import Icon from "../components/Icon.vue";
import ItemArt from "../components/ItemArt.vue";
import SellDialog from "../components/SellDialog.vue";
import PageHeader from "../components/PageHeader.vue";
import PageBar from "../components/PageBar.vue";
import SearchField from "../components/SearchField.vue";
import WfIcon from "../components/WfIcon.vue";
import { inventory, loadInventory, type SellTarget } from "../stores/inventory";
import { ensureRankedQuotes, iconUrl, initMarket, market } from "../stores/market";
import { type InventoryItemDto, type RankedSlug, type VariantDto } from "../lib/api";
import { formatDucats, formatPlatinum } from "../lib/format";
import { useCompactTabs } from "../lib/useCompactTabs";
import { snapshotEnabled } from "../stores/ui";
import type { WfIconName } from "../lib/wficons";

type Category = "all" | "mods" | "arcanes" | "relics" | "parts" | "misc";
type Layout = "list" | "grid";
type SortKey = "value" | "price" | "count" | "ducats" | "name";

const categories: { id: Category; label: string }[] = [
  { id: "all", label: "All" },
  { id: "mods", label: "Mods" },
  { id: "arcanes", label: "Arcanes" },
  { id: "relics", label: "Relics" },
  { id: "parts", label: "Parts" },
  { id: "misc", label: "Misc" },
];

const categoryIcons: Record<Category, WfIconName> = {
  all: "allItems",
  mods: "mods",
  arcanes: "arcanes",
  relics: "relics",
  parts: "primeParts",
  misc: "misc",
};

const LIST_ROW = 56;
const LIST_OVERSCAN = 6;
const GRID_CHUNK = 60;

const category = ref<Category>("all");
const query = ref("");
const ducatsOnly = ref(
  localStorage.getItem("coruframe.inventory.ducatsOnly") === "true",
);
const layout = ref<Layout>(
  localStorage.getItem("coruframe.inventory.layout") === "grid" ? "grid" : "list",
);
const sortKey = ref<SortKey>("value");
const sortDir = ref<"asc" | "desc">("desc");
const sellTarget = ref<SellTarget | null>(null);
const listContainer = ref<HTMLElement | null>(null);
const gridContainer = ref<HTMLElement | null>(null);
const gridSentinel = ref<HTMLElement | null>(null);
const tabsEl = ref<HTMLElement | null>(null);
const tabsCompact = useCompactTabs(tabsEl);
const listScroll = ref(0);
const listViewport = ref(560);
let resizeObserver: ResizeObserver | null = null;
let gridObserver: IntersectionObserver | null = null;
const gridLimit = ref(GRID_CHUNK);


interface LocalPrice {
  platinum: number | null;
  approx: boolean;
  updated_at: number;
  variants: VariantDto[];
}

// Quotes arrive one by one from the market service. Copying them into the
// inventory in 800 ms batches keeps a 650-item queue from re-sorting and
// repainting the list on every event.
const prices = shallowReactive<Record<string, LocalPrice>>({});
let priceTimer: number | null = null;
let scrollIdleTimer: number | null = null;
let scrollFrame: number | null = null;
let pendingListScroll = 0;
let isScrolling = false;
let pricesDirty = false;
let lastVisibleKey = "";

const snapshot = computed(() => inventory.snapshot);
const sellableItems = computed(() =>
  (snapshot.value?.items ?? []).filter((item) => item.sellable),
);
// The filter narrows the rows and the tab counts; the totals stay
// inventory-wide so the value of the whole collection does not jump.
const items = computed(() =>
  ducatsOnly.value
    ? sellableItems.value.filter((item) => (ducatsOf(item) ?? 0) > 0)
    : sellableItems.value,
);

function categoryOf(item: InventoryItemDto): Category {
  if (item.kind === "mod" || item.kind === "upgrade") return "mods";
  if (item.kind === "arcane") return "arcanes";
  if (item.kind === "relic") return "relics";
  if (item.kind === "component" || item.kind === "recipe") return "parts";
  return "misc";
}

function nameOf(item: InventoryItemDto) {
  return item.item.name;
}

function vaultedOf(item: InventoryItemDto) {
  return item.item.vaulted || item.market?.vaulted === true;
}

function ducatsOf(item: InventoryItemDto) {
  return item.market?.ducats ?? null;
}

const categoryCounts = computed(() => {
  const counts: Record<Category, number> = {
    all: items.value.length,
    mods: 0,
    arcanes: 0,
    relics: 0,
    parts: 0,
    misc: 0,
  };
  for (const item of items.value) {
    counts[categoryOf(item)] += 1;
  }
  return counts;
});

// An item with no rank tiers on the market is priced by its headline; only a
// tiered item needs the exact variant. This is what left unranked mods (rank 0
// in the inventory) showing an em dash despite a cached price.
function rankedOf(item: InventoryItemDto) {
  return item.rank != null && item.market?.max_rank != null ? item.rank : null;
}

interface RowPrice {
  platinum: number | null;
  approx: boolean;
  hint: string;
}

function variantRank(variant: VariantDto) {
  return variant.rank ?? 0;
}

// Exact rank first. Intermediate ranks often have no *online* seller at all
// (everyone lists R0 or R10), so the nearest rank keeps the row priced. Those
// values are always marked approximate: the old bug was an R0 stack silently
// valued at R5, and the marker is what stops that from happening again.
function cheapestOf(variants: VariantDto[]): VariantDto | null {
  return variants.reduce<VariantDto | null>(
    (best, variant) =>
      variant.platinum > 0 && (!best || variant.platinum < best.platinum) ? variant : best,
    null,
  );
}

function rowPrice(item: InventoryItemDto): RowPrice {
  const slug = item.market?.slug;
  if (!slug) return { platinum: null, approx: false, hint: "" };
  const quote = prices[slug];
  if (!quote) return { platinum: null, approx: false, hint: "" };
  // A relic family holds one variant per refinement; another refinement's
  // price is a different item and must never stand in for this row.
  const variants = item.subtype
    ? quote.variants.filter((variant) => variant.subtype === item.subtype)
    : quote.variants;
  const rank = rankedOf(item);
  if (rank == null) {
    // The headline is the item's default variant, which is the wrong one for a
    // refinement row; those always come from their own variant.
    if (!item.subtype && quote.platinum != null) {
      return { platinum: quote.platinum, approx: quote.approx, hint: "" };
    }
    const best = cheapestOf(variants);
    return best
      ? { platinum: best.platinum, approx: quote.approx, hint: "" }
      : { platinum: null, approx: false, hint: "" };
  }
  const exact = variants.find((variant) => variant.rank === rank);
  if (exact) return { platinum: exact.platinum, approx: quote.approx, hint: "" };
  let nearest: VariantDto | null = null;
  for (const variant of variants) {
    if (variant.platinum <= 0) continue;
    if (!nearest) {
      nearest = variant;
      continue;
    }
    const distance = Math.abs(variantRank(variant) - rank);
    const best = Math.abs(variantRank(nearest) - rank);
    if (distance < best || (distance === best && variant.platinum < nearest.platinum)) {
      nearest = variant;
    }
  }
  if (!nearest) return { platinum: null, approx: false, hint: "" };
  return {
    platinum: nearest.platinum,
    approx: true,
    hint: `No R${rank} listings; estimated from R${variantRank(nearest)}`,
  };
}

function priceOf(item: InventoryItemDto): number | null {
  return rowPrice(item).platinum;
}

function stackValueOf(item: InventoryItemDto): number | null {
  const price = priceOf(item);
  return price == null ? null : price * item.count;
}

// A row is only requested when it cannot be priced at all; an estimate counts
// as priced. The backend still retries the exact rank on its own cooldown.
function hasQuote(item: InventoryItemDto) {
  return priceOf(item) != null;
}

function hasSetQuote(slug: string) {
  const quote = market.quotes[slug];
  return Boolean(quote && (quote.platinum != null || quote.variants.length > 0));
}


const tracked = computed(() => {
  let total = 0;
  let priced = 0;
  let ducats = 0;
  for (const item of sellableItems.value) {
    const ducat = ducatsOf(item);
    if (ducat != null) ducats += ducat * item.count;
    const value = stackValueOf(item);
    if (value == null) continue;
    total += value;
    priced += 1;
  }
  return {
    total,
    priced,
    ducats,
    stacks: sellableItems.value.length,
  };
});

const indexedPercent = computed(() =>
  tracked.value.stacks
    ? Math.round((tracked.value.priced / tracked.value.stacks) * 100)
    : 0,
);

function priceText(item: InventoryItemDto) {
  const info = rowPrice(item);
  return formatPlatinum(info.platinum, info.approx);
}

function priceHint(item: InventoryItemDto) {
  const info = rowPrice(item);
  if (info.hint) return info.hint;
  return info.approx ? "Approximate price from the recent-orders feed" : "";
}

function setPriceText(item: InventoryItemDto) {
  if (!item.set) return "";
  const quote = prices[item.set.slug];
  if (!quote || quote.platinum == null) return "";
  return formatPlatinum(quote.platinum, quote.approx);
}

function valueText(item: InventoryItemDto) {
  const value = stackValueOf(item);
  if (value == null) return "—";
  return formatPlatinum(value, rowPrice(item).approx);
}

const KIND_LABELS: Record<string, string> = {
  warframe: "Warframe",
  necramech: "Necramech",
  primary: "Primary",
  secondary: "Secondary",
  melee: "Melee",
  companion: "Companion",
  archwing: "Archwing",
  component: "Part",
  mod: "Mod",
  upgrade: "Mod",
  arcane: "Arcane",
  relic: "Relic",
  resource: "Resource",
  fish: "Fish",
  gear: "Gear",
  recipe: "Blueprint",
  misc: "Misc",
};

function kindLabel(item: InventoryItemDto) {
  if (!item.resolved) return "Unknown";
  return KIND_LABELS[item.kind] ?? "Misc";
}

const filtered = computed(() => {
  const search = query.value.trim().toLowerCase();
  return items.value.filter((item) => {
    if (category.value !== "all" && categoryOf(item) !== category.value) return false;
    if (search && !nameOf(item).toLowerCase().includes(search)) return false;
    return true;
  });
});

function sortNumber(item: InventoryItemDto, key: SortKey): number | null {
  if (key === "count") return item.count;
  if (key === "ducats") return ducatsOf(item);
  if (key === "price") return priceOf(item);
  return stackValueOf(item);
}

const sorted = computed(() => {
  const list = [...filtered.value];
  const direction = sortDir.value === "asc" ? 1 : -1;
  list.sort((left, right) => {
    if (sortKey.value === "name") {
      return nameOf(left).localeCompare(nameOf(right)) * direction;
    }
    const leftValue = sortNumber(left, sortKey.value);
    const rightValue = sortNumber(right, sortKey.value);
    if (leftValue == null && rightValue == null) {
      return nameOf(left).localeCompare(nameOf(right));
    }
    if (leftValue == null) return 1;
    if (rightValue == null) return -1;
    if (leftValue === rightValue) {
      return nameOf(left).localeCompare(nameOf(right));
    }
    return (leftValue - rightValue) * direction;
  });
  return list;
});

const listStart = computed(() =>
  Math.max(0, Math.floor(listScroll.value / LIST_ROW) - LIST_OVERSCAN),
);
const listEnd = computed(() =>
  Math.min(
    sorted.value.length,
    Math.ceil((listScroll.value + listViewport.value) / LIST_ROW) + LIST_OVERSCAN,
  ),
);
const listItems = computed(() =>
  sorted.value.slice(listStart.value, listEnd.value),
);
const listTop = computed(() => listStart.value * LIST_ROW);
const listBottom = computed(() =>
  Math.max(0, (sorted.value.length - listEnd.value) * LIST_ROW),
);

// shortcut: the gallery keeps every loaded card mounted, so a full inventory
// scroll depends on the browser's own layout; switch to a measured row
// virtualizer if that becomes slow.
const gridItems = computed(() => sorted.value.slice(0, gridLimit.value));

// Cards are auto-height, so the gallery cannot measure rows; it appends a
// chunk whenever the sentinel under the last card comes near the viewport.
// The observer is rebuilt after each append so an append that leaves the
// sentinel visible keeps filling instead of stalling.
function observeGrid() {
  gridObserver?.disconnect();
  gridObserver = null;
  if (layout.value !== "grid") return;
  const root = gridContainer.value;
  const sentinel = gridSentinel.value;
  if (!root || !sentinel) return;
  gridObserver = new IntersectionObserver(
    (entries) => {
      if (!entries.some((entry) => entry.isIntersecting)) return;
      if (gridLimit.value >= sorted.value.length) return;
      gridLimit.value = Math.min(sorted.value.length, gridLimit.value + GRID_CHUNK);
      void nextTick(observeGrid);
    },
    { root, rootMargin: "600px" },
  );
  gridObserver.observe(sentinel);
}

function measure() {
  if (listContainer.value) {
    listViewport.value = listContainer.value.clientHeight;
  }
}

// Rows the user has actually seen this session. They are revalidated once;
// scrolling back must not turn the view into a request loop.
const validated = new Set<string>();

function pairKey(slug: string, rank: number | null, subtype: string | null) {
  return `${slug}\u0000${rank ?? "all"}\u0000${subtype ?? "any"}`;
}

function requestVisibleQuotes() {
  const visible = new Map<string, RankedSlug>();
  const refresh = new Map<string, RankedSlug>();
  const rendered = layout.value === "list" ? listItems.value : gridItems.value;
  for (const item of rendered) {
    if (item.sellable && item.market) {
      const key = pairKey(item.market.slug, item.rank, item.subtype);
      const pair = { slug: item.market.slug, rank: item.rank, subtype: item.subtype };
      if (!hasQuote(item)) visible.set(key, pair);
      if (!validated.has(key)) {
        validated.add(key);
        refresh.set(key, pair);
      }
    }
    if (item.set) {
      const key = pairKey(item.set.slug, null, null);
      const pair = { slug: item.set.slug, rank: null, subtype: null };
      if (!hasSetQuote(item.set.slug)) visible.set(key, pair);
      if (!validated.has(key)) {
        validated.add(key);
        refresh.set(key, pair);
      }
    }
  }
  // The screen's prices revalidate right away instead of waiting out the
  // six-hour cache; the background sweep keeps handling everything else.
  if (refresh.size) void ensureRankedQuotes([...refresh.values()], true);
  const pairs = [...visible.values()].sort(
    (left, right) =>
      left.slug.localeCompare(right.slug) ||
      (left.rank ?? -1) - (right.rank ?? -1) ||
      (left.subtype ?? "").localeCompare(right.subtype ?? ""),
  );
  const key = pairs.map((pair) => pairKey(pair.slug, pair.rank, pair.subtype)).join(",");
  if (!pairs.length || key === lastVisibleKey) return;
  lastVisibleKey = key;
  void ensureRankedQuotes(pairs);
}

function markScrolling() {
  isScrolling = true;
  if (scrollIdleTimer != null) window.clearTimeout(scrollIdleTimer);
  scrollIdleTimer = window.setTimeout(() => {
    scrollIdleTimer = null;
    isScrolling = false;
    if (pricesDirty) {
      pricesDirty = false;
      syncPrices();
    }
    requestVisibleQuotes();
  }, 150);
}

function flushScroll() {
  scrollFrame = null;
  listScroll.value = pendingListScroll;
}

function onListScroll(event: Event) {
  // Measure and resize stay in measure(); one ref write per frame is enough.
  pendingListScroll = (event.target as HTMLElement).scrollTop;
  markScrolling();
  if (scrollFrame == null) scrollFrame = requestAnimationFrame(flushScroll);
}

function resetScroll() {
  if (scrollFrame != null) {
    cancelAnimationFrame(scrollFrame);
    scrollFrame = null;
  }
  pendingListScroll = 0;
  listScroll.value = 0;
  gridLimit.value = GRID_CHUNK;
  if (listContainer.value) listContainer.value.scrollTop = 0;
  if (gridContainer.value) gridContainer.value.scrollTop = 0;
}

watch([category, query, sortKey, sortDir, layout, ducatsOnly], () => {
  resetScroll();
  requestAnimationFrame(requestVisibleQuotes);
  void nextTick(observeGrid);
});
watch(layout, (value) => {
  localStorage.setItem("coruframe.inventory.layout", value);
  requestAnimationFrame(measure);
});
watch(ducatsOnly, (value) => {
  localStorage.setItem("coruframe.inventory.ducatsOnly", String(value));
});
function syncPrices() {
  const slugs = new Set<string>();
  for (const item of sellableItems.value) {
    if (item.market) slugs.add(item.market.slug);
    if (item.set) slugs.add(item.set.slug);
  }
  for (const slug of slugs) {
    const quote = market.quotes[slug];
    const next = {
      platinum: quote?.platinum ?? null,
      approx: quote?.approx ?? false,
      updated_at: quote?.updated_at ?? 0,
      variants: quote?.variants ?? [],
    };
    const previous = prices[slug];
    if (
      previous &&
      previous.platinum === next.platinum &&
      previous.approx === next.approx &&
      previous.updated_at === next.updated_at
    ) {
      continue;
    }
    prices[slug] = next;
  }
}

watch(
  snapshot,
  () => {
    syncPrices();
    requestVisibleQuotes();
  },
  { immediate: true },
);

// The bulk cached read usually lands after the snapshot watcher; applying it
// immediately means the first painted order is the real one.
watch(
  () => inventory.pricesReady,
  (ready) => {
    if (!ready) return;
    if (priceTimer != null) {
      window.clearTimeout(priceTimer);
      priceTimer = null;
    }
    syncPrices();
    requestVisibleQuotes();
  },
);

watch(
  () => market.version,
  () => {
    // Rewriting prices re-sorts the list; hold them until the gesture ends.
    if (isScrolling) {
      pricesDirty = true;
      return;
    }
    if (priceTimer != null) return;
    priceTimer = window.setTimeout(() => {
      priceTimer = null;
      if (isScrolling) {
        pricesDirty = true;
        return;
      }
      syncPrices();
    }, 800);
  },
);

onUnmounted(() => {
  if (priceTimer != null) window.clearTimeout(priceTimer);
  if (scrollIdleTimer != null) window.clearTimeout(scrollIdleTimer);
  if (scrollFrame != null) cancelAnimationFrame(scrollFrame);
  resizeObserver?.disconnect();
  gridObserver?.disconnect();
});

function openItem(item: InventoryItemDto) {
  if (!item.sellable || !item.market) return;
  sellTarget.value = {
    market: item.market,
    name: item.item.name,
    count: item.count,
    rank: item.rank,
    subtype: item.subtype,
  };
}


function marketIconOf(item: InventoryItemDto) {
  return item.market ? iconUrl(item.market.thumb ?? item.market.icon) : null;
}

onMounted(() => {
  void loadInventory();
  void initMarket();
  requestAnimationFrame(() => {
    measure();
    const observer = new ResizeObserver(measure);
    if (listContainer.value) observer.observe(listContainer.value);
    resizeObserver = observer;
    observeGrid();
    requestVisibleQuotes();
  });
});
</script>

<template>
  <div class="view">
    <PageBar>
      <template #actions>
        <SearchField v-model="query" placeholder="Search inventory…" label="Search inventory" />

        <label class="select-field">
          <Icon name="stats" :size="15" />
          <select v-model="sortKey" aria-label="Sort inventory">
            <option value="value">Stack value</option>
            <option value="price">Unit price</option>
            <option value="count">Owned</option>
            <option value="ducats">Ducats</option>
            <option value="name">Name</option>
          </select>
        </label>
        <button
          class="icon-button"
          type="button"
          :aria-label="sortDir === 'asc' ? 'Ascending' : 'Descending'"
          @click="sortDir = sortDir === 'asc' ? 'desc' : 'asc'"
        >
          <Icon :name="sortDir === 'asc' ? 'arrowUp' : 'arrowDown'" :size="18" />
        </button>

        <button
          class="ducats-toggle"
          type="button"
          :aria-pressed="ducatsOnly"
          title="Show only items with a ducat value"
          @click="ducatsOnly = !ducatsOnly"
        >
          <CurrencyIcon name="ducats" :size="14" />
          <span>Ducats only</span>
        </button>

        <div class="inventory-layout-toggle">
          <button
            class="icon-button"
            :title="layout === 'grid' ? 'Switch to list view' : 'Switch to grid view'"
            :aria-label="layout === 'grid' ? 'Switch to list view' : 'Switch to grid view'"
            type="button"
            @click="layout = layout === 'grid' ? 'list' : 'grid'"
          >
            <Icon :name="layout === 'grid' ? 'list' : 'grid'" :size="18" />
          </button>
        </div>

        <span class="muted inventory-count">{{ sorted.length }} items</span>
      </template>

      <template #tabs>
        <div
          ref="tabsEl"
          class="inventory-tabs"
          :class="{ compact: tabsCompact }"
          role="tablist"
          aria-label="Inventory section"
        >
          <button
            v-for="option in categories"
            :key="option.id"
            type="button"
            role="tab"
            class="tier-tab"
            :class="{ active: category === option.id }"
            :aria-selected="category === option.id"
            :aria-label="option.label"
            :title="option.label"
            @click="category = option.id"
          >
            <WfIcon :name="categoryIcons[option.id]" :size="16" />
            <span class="tab-label">{{ option.label }}</span>
            <span class="tab-count">{{ categoryCounts[option.id] }}</span>
          </button>
        </div>
      </template>
    </PageBar>

    <div class="content">
      <PageHeader
        title="Inventory"
        description="Sellable items and market value."
        icon="inventory"
      >
        <template #actions>
          <span v-if="inventory.pricesError" class="preview-chip warn" :title="inventory.pricesError">
            <Icon name="database" :size="13" /> Cached prices unavailable
          </span>
        </template>
      </PageHeader>

      <div v-if="!snapshotEnabled" class="panel empty-inventory">
        <Icon name="lock" :size="30" />
        <h2>Snapshot disabled</h2>
        <p>Enable the inventory snapshot in Settings to use the inventory and sell planner.</p>
        <RouterLink to="/settings" class="button ghost">Open settings</RouterLink>
      </div>
      <div v-else-if="inventory.loading && !snapshot" class="panel empty-inventory">
        <Icon name="clock" :size="30" />
        <h2>Loading inventory snapshot…</h2>
        <p>Reading the local snapshot…</p>
      </div>
      <div v-else-if="!snapshot?.linked" class="panel empty-inventory">
        <Icon name="database" :size="30" />
        <h2>No inventory snapshot yet</h2>
        <p>
          Launch Warframe through the wrapper, open the in-game inventory, and wait for the first scan.
        </p>
        <RouterLink to="/settings" class="button ghost">Open settings</RouterLink>
      </div>
      <div v-else-if="!inventory.pricesReady" class="panel empty-inventory">
        <Icon name="trading" :size="30" />
        <h2>Pricing the inventory…</h2>
        <p>Loading cached prices for {{ snapshot.sellable }} items.</p>
      </div>

      <template v-else>
        <div class="summary-row inventory-summary">
          <div class="summary-card tone-gold">
            <span class="summary-icon"><CurrencyIcon name="platinum" :size="18" /></span>
            <div>
              <strong>{{ formatPlatinum(tracked.total) }}</strong>
              <span>Total sellable value</span>
            </div>
          </div>
          <div class="summary-card tone-blue">
            <span class="summary-icon"><CurrencyIcon name="ducats" :size="18" /></span>
            <div>
              <strong>{{ formatDucats(tracked.ducats) }}</strong>
              <span>Total ducat value</span>
            </div>
          </div>
          <span
            class="indexed-indicator"
            :title="`${tracked.priced} of ${tracked.stacks} sellable stacks priced`"
          >
            <strong>{{ indexedPercent }}%</strong> indexed
          </span>
        </div>

        <div v-if="!sorted.length" class="panel empty-inventory">
          <Icon name="search" :size="30" />
          <h2>{{ ducatsOnly ? "No ducat items" : "No items match" }}</h2>
          <p>
            {{
              ducatsOnly
                ? "No sellable item in this category has a ducat value."
                : "Try a different search, category, or sort."
            }}
          </p>
          <button
            v-if="ducatsOnly"
            class="button ghost"
            type="button"
            @click="ducatsOnly = false"
          >
            Clear filter
          </button>
        </div>

        <div v-else class="panel inventory-panel">
          <div
            v-if="layout === 'list'"
            ref="listContainer"
            class="data-table-wrap inventory-table-wrap inventory-scroll"
            @scroll.passive="onListScroll"
          >
            <table class="data-table">
              <thead>
                <tr>
                  <th>Item</th>
                  <th>Kind</th>
                  <th class="num">Owned</th>
                  <th class="num">Unit</th>
                  <th class="num">Stack value</th>
                  <th class="num">Ducats</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                <tr
                  v-if="listTop"
                  class="virtual-spacer"
                  :style="{ height: `${listTop}px` }"
                >
                  <td colspan="7"></td>
                </tr>
                <tr v-for="item in listItems" :key="`${item.item.unique_name}-${item.rank ?? 'none'}`">
                  <td>
                    <div class="inventory-name">
                      <span class="inventory-icon">
                        <ItemArt
                          :image="item.item.image_url"
                          :market-icon="marketIconOf(item)"
                          :market-first="item.item.kind === 'component'"
                          :kind="item.item.kind"
                          :icon-size="16"
                          :alt="nameOf(item)"
                        />
                      </span>
                      <span>
                        <strong class="selectable">{{ nameOf(item) }}</strong>
                        <span class="inventory-meta">
                          <span v-if="item.rank != null" class="badge neutral">R{{ item.rank }}</span>
                          <span v-if="vaultedOf(item)" class="badge vaulted">Vaulted</span>

                          <span v-if="!item.resolved" class="badge neutral" title="Not in the item database">
                            Unknown
                          </span>
                          <span
                            v-if="snapshot.mastery_available && item.mastered"
                            class="badge owned"
                          >
                            Mastered
                          </span>
                        </span>
                        <span v-if="item.set" class="inventory-set-line">
                          Part of {{ item.set.name }} · {{ item.set.have }}/{{ item.set.need }} parts
                          <template v-if="setPriceText(item)"> · set {{ setPriceText(item) }}</template>
                        </span>
                      </span>
                    </div>
                  </td>
                  <td class="muted">{{ kindLabel(item) }}</td>
                  <td class="num">{{ item.count }}</td>
                  <td class="num selectable" :title="priceHint(item)">{{ priceText(item) }}</td>
                  <td class="num selectable" :title="priceHint(item)">{{ valueText(item) }}</td>
                  <td class="num selectable">
                    {{ formatDucats(ducatsOf(item)) }}
                  </td>
                  <td class="num">
                    <button
                      v-if="item.sellable"
                      class="button small"
                      type="button"
                      @click="openItem(item)"
                    >
                      <Icon name="trading" :size="12" /> Sell
                    </button>
                    <span v-else class="muted">—</span>
                  </td>
                </tr>
                <tr
                  v-if="listBottom"
                  class="virtual-spacer"
                  :style="{ height: `${listBottom}px` }"
                >
                  <td colspan="7"></td>
                </tr>
              </tbody>
            </table>
          </div>

          <div
            v-else
            ref="gridContainer"
            class="inventory-grid-wrap inventory-scroll"
          >
            <div class="inventory-grid">
              <article
                v-for="item in gridItems"
                :key="`${item.item.unique_name}-${item.rank ?? 'none'}-${item.subtype ?? 'any'}`"
                class="inventory-card"
              >
              <div class="inventory-card-head">
                <span class="inventory-icon">
                  <ItemArt
                    :image="item.item.image_url"
                    :market-icon="marketIconOf(item)"
                    :market-first="item.item.kind === 'component'"
                    :kind="item.item.kind"
                    :icon-size="18"
                    :alt="nameOf(item)"
                  />
                </span>
                <div class="inventory-card-title">
                  <h3 :title="nameOf(item)" class="selectable">{{ nameOf(item) }}</h3>
                  <span>{{ kindLabel(item) }}</span>
                </div>
                <span v-if="item.rank != null" class="badge neutral">R{{ item.rank }}</span>
              </div>
              <div class="inventory-card-meta">
                <span>Owned <strong>{{ item.count }}</strong></span>
                <span class="selectable" :title="priceHint(item)">{{ priceText(item) }}</span>
                <span class="inventory-stack-value selectable" :title="priceHint(item)">{{ valueText(item) }}</span>
                <span v-if="ducatsOf(item)" class="selectable">{{ formatDucats(ducatsOf(item)) }}</span>
              </div>
              <div v-if="item.set" class="inventory-set-line">
                Part of {{ item.set.name }} · {{ item.set.have }}/{{ item.set.need }} parts
                <template v-if="setPriceText(item)"> · set {{ setPriceText(item) }}</template>
              </div>
              <div class="inventory-card-foot">
                <span v-if="vaultedOf(item)" class="badge vaulted">Vaulted</span>

                <span v-if="!item.resolved" class="badge neutral">Unknown</span>
                <span
                  v-if="snapshot.mastery_available && item.mastered"
                  class="badge owned"
                >
                  Mastered
                </span>
                <button
                  v-if="item.sellable"
                  class="button small"
                  type="button"
                  @click="openItem(item)"
                >
                  <Icon name="trading" :size="12" /> Sell
                </button>
              </div>
              </article>
            </div>
            <div ref="gridSentinel" class="inventory-grid-sentinel"></div>
          </div>

        </div>

      </template>
    </div>

    <SellDialog v-if="sellTarget" :target="sellTarget" @close="sellTarget = null" />
  </div>
</template>
