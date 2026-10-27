<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import CurrencyIcon from "./CurrencyIcon.vue";
import Icon from "./Icon.vue";
import Sparkline from "./Sparkline.vue";
import {
  clearActive,
  focusItem,
  iconUrl,
  initMarket,
  isTracked,
  market,
  quoteDelta,
  relativeTime,
  searchMarket,
  track,
  untrack,
} from "../stores/market";
import { openExternal } from "../lib/api";
import { formatCount, formatDucats, formatPlatinum, formatRank } from "../lib/format";
import type { MarketItemDto, OrderDto } from "../lib/api";

const copiedKey = ref("");
let searchTimer: number | null = null;
let copyTimer: number | null = null;

const activeItem = computed(() => market.focus?.item ?? market.active ?? null);
const activeQuote = computed(() =>
  activeItem.value ? market.quotes[activeItem.value.slug] ?? null : null,
);
const delta = computed(() => quoteDelta(activeQuote.value));
const history = computed(() => activeQuote.value?.history ?? []);
const priceLabel = computed(() => {
  const quote = activeQuote.value;
  if (!quote || quote.platinum == null) return null;
  return `${quote.approx ? "~" : ""}${quote.platinum}`;
});
const trackedItems = computed<MarketItemDto[]>(() => {
  const items: MarketItemDto[] = [];
  for (const slug of market.tracked) {
    const item = market.items[slug];
    if (item) items.push(item);
  }
  return items;
});
const showResults = computed(() => market.query.trim().length > 0);

function itemsForList(): MarketItemDto[] {
  return showResults.value ? market.results : trackedItems.value;
}
const sellOrders = computed(() => market.focus?.sell ?? []);
const buyOrders = computed(() => market.focus?.buy ?? []);
const whisperOrder = computed(() => market.focus?.best_sell ?? null);

const selectedRank = ref<number | null>(null);
const selectedCharges = ref<number | null>(null);
const selectedAmber = ref<number | null>(null);
const selectedCyan = ref<number | null>(null);
const selectedSubtype = ref<string | null>(null);
let lastSlug: string | null = null;

const rankOptions = computed(() => {
  const max = activeItem.value?.max_rank;
  return max != null && max > 0
    ? Array.from({ length: max + 1 }, (_, index) => index)
    : [];
});
const chargeOptions = computed(() => {
  const max = activeItem.value?.max_charges;
  return max != null && max > 0
    ? Array.from({ length: max + 1 }, (_, index) => index)
    : [];
});
const amberOptions = computed(() => {
  const max = activeItem.value?.max_amber_stars;
  return max != null && max > 0
    ? Array.from({ length: max + 1 }, (_, index) => index)
    : [];
});
const cyanOptions = computed(() => {
  const max = activeItem.value?.max_cyan_stars;
  return max != null && max > 0
    ? Array.from({ length: max + 1 }, (_, index) => index)
    : [];
});
const subtypeOptions = computed(() => activeItem.value?.subtypes ?? []);

function onSearch() {
  if (searchTimer) window.clearTimeout(searchTimer);
  searchTimer = window.setTimeout(() => {
    void searchMarket(market.query);
  }, 250);
}

function priceText(quote: { platinum: number | null; approx: boolean } | null | undefined) {
  return formatPlatinum(quote?.platinum, quote?.approx);
}

function onlineText(quote: { online: number; approx: boolean } | null | undefined) {
  if (!quote) return "";
  return `${quote.approx ? "~" : ""}${quote.online}`;
}

function deltaFor(item: MarketItemDto) {
  return quoteDelta(market.quotes[item.slug]);
}

function historyFor(item: MarketItemDto) {
  const quote = market.quotes[item.slug];
  return quote && quote.history.length > 1 ? quote.history : [1, 1];
}

function toneFor(item: MarketItemDto) {
  const change = deltaFor(item);
  return change > 0 ? "up" : change < 0 ? "down" : "flat";
}

function formatDucatValue(item: MarketItemDto) {
  return formatDucats(item.ducats);
}

function openMarket(slug: string) {
  void openExternal(`https://warframe.market/items/${slug}`);
}

function trackToggle(item: MarketItemDto) {
  if (isTracked(item.slug)) {
    untrack(item.slug);
  } else {
    void track(item.slug);
  }
}

function orderKey(order: OrderDto, side: "buy" | "sell") {
  return `${side}-${order.ingame_name}-${order.platinum}`;
}

function isCopied(order: OrderDto | null, side: "buy" | "sell") {
  return Boolean(order && copiedKey.value === orderKey(order, side));
}

function copyWhisper(order: OrderDto | null, side: "buy" | "sell") {
  if (!activeItem.value || !order) return;
  const verb = side === "buy" ? "buy" : "sell";
  const text = `/w ${order.ingame_name} Hi! I want to ${verb}: "${activeItem.value.name}" for ${order.unit_price} platinum. (warframe.market via CoruFrame)`;
  void navigator.clipboard?.writeText(text).then(() => {
    copiedKey.value = orderKey(order, side);
    if (copyTimer) window.clearTimeout(copyTimer);
    copyTimer = window.setTimeout(() => (copiedKey.value = ""), 1600);
  });
}

function orderAttrs(order: OrderDto) {
  const attrs: { key: string; icon?: "zap" | "amberStar" | "cyanStar"; text: string }[] = [];
  if (order.rank != null) attrs.push({ key: "rank", text: formatRank(order.rank) });
  if (order.charges != null) attrs.push({ key: "charges", icon: "zap", text: String(order.charges) });
  if (order.amber_stars != null) {
    attrs.push({ key: "amber", icon: "amberStar", text: String(order.amber_stars) });
  }
  if (order.cyan_stars != null) {
    attrs.push({ key: "cyan", icon: "cyanStar", text: String(order.cyan_stars) });
  }
  if (order.subtype) attrs.push({ key: "subtype", text: order.subtype });
  if (order.per_trade > 1) {
    attrs.push({
      key: "trade",
      text: `${order.per_trade}/trade · ${formatPlatinum(order.platinum)}/trade`,
    });
  }
  return attrs;
}

function filterValue() {
  return {
    rank: selectedRank.value,
    charges: selectedCharges.value,
    amber_stars: selectedAmber.value,
    cyan_stars: selectedCyan.value,
    subtype: selectedSubtype.value,
  };
}

function applyFilter() {
  if (!activeItem.value) return;
  void focusItem(activeItem.value, filterValue());
}

function refresh() {
  if (!activeItem.value) return;
  void focusItem(activeItem.value, filterValue(), true);
}

watch(
  () => activeItem.value,
  (item) => {
    if (!item) {
      lastSlug = null;
      return;
    }
    if (item.slug === lastSlug) return;
    lastSlug = item.slug;
    const filter = market.activeFilter;
    selectedRank.value = filter?.rank ?? null;
    selectedCharges.value = filter?.charges ?? null;
    selectedAmber.value = filter?.amber_stars ?? null;
    selectedCyan.value = filter?.cyan_stars ?? null;
    selectedSubtype.value = filter?.subtype ?? null;
  },
  { immediate: true },
);

onMounted(async () => {
  await initMarket();
});
</script>

<template>
  <div class="rail-panel market-panel">
    <label class="search-field rail-search">
      <Icon name="search" :size="15" />
      <input
        v-model="market.query"
        type="search"
        placeholder="Search warframe.market…"
        aria-label="Search warframe.market"
        spellcheck="false"
        autocomplete="off"
        autocapitalize="off"
        @input="onSearch"
      />
      <span v-if="market.loading" class="market-spinner" aria-label="Searching"></span>
    </label>

    <div v-if="market.error" class="market-error">{{ market.error }}</div>

    <div v-if="activeItem" class="market-focus">
      <div class="market-focus-top">
        <div class="market-focus-title">
          <span v-if="iconUrl(activeItem.icon)" class="market-item-icon">
            <img
              :src="iconUrl(activeItem.icon)!"
              :alt="activeItem.name"
              referrerpolicy="no-referrer"
              loading="lazy"
              decoding="async"
              draggable="false"
            />
          </span>
          <div>
            <div class="eyebrow">Lowest online sell</div>
            <h2 class="selectable">{{ activeItem.name }}</h2>
          </div>
        </div>
        <button class="icon-button small" type="button" aria-label="Clear selection" @click="clearActive">
          <Icon name="x" :size="14" />
        </button>
      </div>

      <div v-if="priceLabel" class="market-focus-price">
        {{ priceLabel }}<CurrencyIcon name="platinum" :size="15" class="market-price-unit" />
      </div>
      <div v-else class="market-focus-price market-focus-empty">
        {{ market.focusLoading ? "…" : "—" }}
      </div>

      <div class="market-focus-meta">
        <span class="online-dot"></span>{{ onlineText(activeQuote) }} online
        <span v-if="activeQuote?.approx" class="muted">recent</span>
        <span v-if="delta" class="delta" :class="{ up: delta > 0, down: delta < 0 }">
          <Icon :name="delta >= 0 ? 'arrowUp' : 'arrowDown'" :size="11" />
          {{ Math.abs(delta).toFixed(1) }}%
        </span>
        <span v-if="activeItem.vaulted" class="badge vaulted"><Icon name="lock" :size="10" /> Vaulted</span>
        <span v-else class="badge neutral">Current</span>
      </div>
      <div class="market-focus-sub">
        <CurrencyIcon name="ducats" :size="11" />
        {{ formatDucatValue(activeItem) }} · {{ activeQuote ? relativeTime(activeQuote.updated_at) : "waiting for quote" }}
      </div>

      <div v-if="history.length > 1" class="market-focus-chart">
        <Sparkline :values="history" :width="264" :height="42" :tone="delta >= 0 ? 'up' : 'down'" />
      </div>

      <div
        v-if="rankOptions.length || chargeOptions.length || amberOptions.length || cyanOptions.length || subtypeOptions.length"
        class="order-filters"
      >
        <label v-if="rankOptions.length" class="order-filter">
          <span>Rank</span>
          <select v-model.number="selectedRank" @change="applyFilter">
            <option v-for="value in rankOptions" :key="value" :value="value">
              {{ value === activeItem.max_rank ? `R${value} · max` : `R${value}` }}
            </option>
          </select>
        </label>
        <label v-if="chargeOptions.length" class="order-filter">
          <span>Charges</span>
          <select v-model.number="selectedCharges" @change="applyFilter">
            <option v-for="value in chargeOptions" :key="value" :value="value">{{ value }}</option>
          </select>
        </label>
        <label v-if="amberOptions.length" class="order-filter">
          <span>Amber stars</span>
          <select v-model.number="selectedAmber" @change="applyFilter">
            <option v-for="value in amberOptions" :key="value" :value="value">{{ value }}</option>
          </select>
        </label>
        <label v-if="cyanOptions.length" class="order-filter">
          <span>Cyan stars</span>
          <select v-model.number="selectedCyan" @change="applyFilter">
            <option v-for="value in cyanOptions" :key="value" :value="value">{{ value }}</option>
          </select>
        </label>
        <label v-if="subtypeOptions.length" class="order-filter">
          <span>Subtype</span>
          <select v-model="selectedSubtype" @change="applyFilter">
            <option :value="null">Any</option>
            <option v-for="value in subtypeOptions" :key="value" :value="value">{{ value }}</option>
          </select>
        </label>
      </div>

      <div class="market-focus-actions">
        <button class="button primary" type="button" @click="openMarket(activeItem.slug)">
          <Icon name="external" :size="13" /> Open market
        </button>
        <button class="button ghost" type="button" :disabled="!whisperOrder" @click="copyWhisper(whisperOrder, 'buy')">
          <Icon name="copy" :size="13" /> {{ isCopied(whisperOrder, 'buy') ? "Copied" : "Whisper" }}
        </button>
      </div>
      <div class="market-focus-actions secondary">
        <button class="button ghost" type="button" @click="trackToggle(activeItem)">
          <Icon :name="isTracked(activeItem.slug) ? 'check' : 'plus'" :size="13" />
          {{ isTracked(activeItem.slug) ? "Tracked" : "Track" }}
        </button>
        <button class="button ghost" type="button" :disabled="market.focusLoading" @click="refresh">
          <Icon name="refresh" :size="13" /> Refresh
        </button>
      </div>
    </div>

    <template v-if="activeItem">
      <div v-if="!market.focus" class="market-empty">
        {{ market.focusLoading ? "Loading the order book…" : "No order book yet." }}
      </div>
      <template v-else>
        <div class="rail-section-head">
          <h2>Sell orders</h2>
          <span class="muted">{{ sellOrders.length }} online</span>
        </div>
        <div class="market-orders">
          <div
            v-for="order in sellOrders"
            :key="`sell-${order.ingame_name}-${order.platinum}`"
            class="market-order"
          >
            <span class="market-order-status" :class="order.status"></span>
            <div class="market-order-main">
              <span class="market-order-name selectable">{{ order.ingame_name }}</span>
              <span v-if="orderAttrs(order).length" class="market-order-attrs">
                <template v-for="attr in orderAttrs(order)" :key="attr.key">
                  <span v-if="attr.icon" class="order-attr">
                    <Icon :name="attr.icon" :size="10" :class="`order-attr-${attr.key}`" />{{
                      attr.text
                    }}
                  </span>
                  <span v-else class="order-attr">{{ attr.text }}</span>
                </template>
              </span>
            </div>
            <span class="market-order-qty">{{ formatCount(order.quantity) }}</span>
            <span class="market-order-price selectable">{{ formatPlatinum(order.unit_price) }}</span>
            <button
              class="market-order-copy"
              :class="{ copied: isCopied(order, 'buy') }"
              type="button"
              :aria-label="`Copy buy whisper to ${order.ingame_name}`"
              :title="isCopied(order, 'buy') ? 'Copied' : `Copy whisper to ${order.ingame_name}`"
              @click="copyWhisper(order, 'buy')"
            >
              <Icon :name="isCopied(order, 'buy') ? 'check' : 'copy'" :size="12" />
            </button>
          </div>
          <div v-if="!sellOrders.length" class="market-empty">No online sell orders.</div>
        </div>

        <div class="rail-section-head">
          <h2>Buy orders</h2>
          <span class="muted">Top</span>
        </div>
        <div class="market-orders">
          <div
            v-for="order in buyOrders"
            :key="`buy-${order.ingame_name}-${order.platinum}`"
            class="market-order buy"
          >
            <span class="market-order-status" :class="order.status"></span>
            <div class="market-order-main">
              <span class="market-order-name selectable">{{ order.ingame_name }}</span>
              <span v-if="orderAttrs(order).length" class="market-order-attrs">
                <template v-for="attr in orderAttrs(order)" :key="attr.key">
                  <span v-if="attr.icon" class="order-attr">
                    <Icon :name="attr.icon" :size="10" :class="`order-attr-${attr.key}`" />{{
                      attr.text
                    }}
                  </span>
                  <span v-else class="order-attr">{{ attr.text }}</span>
                </template>
              </span>
            </div>
            <span class="market-order-qty">{{ formatCount(order.quantity) }}</span>
            <span class="market-order-price selectable">{{ formatPlatinum(order.unit_price) }}</span>
            <button
              class="market-order-copy"
              :class="{ copied: isCopied(order, 'sell') }"
              type="button"
              :aria-label="`Copy sell whisper to ${order.ingame_name}`"
              :title="isCopied(order, 'sell') ? 'Copied' : `Copy whisper to ${order.ingame_name}`"
              @click="copyWhisper(order, 'sell')"
            >
              <Icon :name="isCopied(order, 'sell') ? 'check' : 'copy'" :size="12" />
            </button>
          </div>
          <div v-if="!buyOrders.length" class="market-empty">No online buy orders.</div>
        </div>
      </template>
    </template>

    <template v-if="!activeItem">
      <div class="rail-section-head">
        <h2>{{ showResults ? "Search results" : "Tracked items" }}</h2>
        <span class="muted">{{ showResults ? market.results.length : trackedItems.length }}</span>
      </div>
      <div class="market-list">
      <button
        v-for="item in itemsForList()"
        :key="item.slug"
        type="button"
        class="market-row"
        @click="focusItem(item)"
      >
        <span class="market-row-name">{{ item.name }}</span>
        <Sparkline :values="historyFor(item)" :tone="toneFor(item)" />
        <span class="market-row-price">{{ priceText(market.quotes[item.slug]) }}</span>
        <span class="market-row-online">{{ onlineText(market.quotes[item.slug]) }}</span>
        <span class="delta" :class="{ up: deltaFor(item) > 0, down: deltaFor(item) < 0 }">
          {{ deltaFor(item) ? `${deltaFor(item) > 0 ? "+" : ""}${deltaFor(item).toFixed(1)}%` : "—" }}
        </span>
      </button>
      <div v-if="!showResults && !trackedItems.length" class="market-empty">
        Search for an item or track one from the results.
      </div>
      <div v-if="showResults && !market.results.length && !market.loading" class="market-empty">
        No matching items.
      </div>
    </div>

    <div class="rail-section-head">
      <h2>Trending now</h2>
      <span class="muted">recent orders</span>
    </div>
    <div class="market-list">
      <button
        v-for="row in market.trending.slice(0, 8)"
        :key="`trending-${row.item.slug}`"
        type="button"
        class="market-row trending-row"
        :title="`${row.orders} recent orders · ${row.quantity} listed`"
        @click="focusItem(row.item)"
      >
        <span class="market-row-name">{{ row.item.name }}</span>
        <span class="trending-orders">{{ formatCount(row.orders) }}</span>
        <span class="market-row-price">{{ formatPlatinum(row.min_sell) }}</span>
      </button>
        <div v-if="!market.trending.length" class="market-empty">Waiting for recent orders…</div>
      </div>
    </template>

    <div class="rail-foot">
      <Icon name="signal" :size="12" />
      warframe.market · {{ market.status?.cached ?? 0 }} cached
      <span
        v-if="market.status?.last_error"
        class="market-error-dot"
        :title="market.status.last_error"
      ></span>
    </div>
  </div>
</template>
