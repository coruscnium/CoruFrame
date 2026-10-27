<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { RouterLink } from "vue-router";
import Icon from "../components/Icon.vue";
import PageHeader from "../components/PageHeader.vue";
import PageBar from "../components/PageBar.vue";
import SearchField from "../components/SearchField.vue";
import { inventory, loadInventory } from "../stores/inventory";
import {
  ensureQuotes,
  initMarket,
  market,
} from "../stores/market";
import { formatCount, formatPlatinum, formatRank } from "../lib/format";
import { useCompactTabs } from "../lib/useCompactTabs";
import {
  account,
  closeOrder,
  deleteOrder,
  loadAccount,
  openSellCount,
  listedValue,
  refreshOrders,
  setVisible,
  updateOrder,
} from "../stores/account";
import type { MyOrderDto } from "../lib/api";

type Tab = "sell" | "buy";

const tab = ref<Tab>("sell");
const query = ref("");
const editingId = ref<string | null>(null);
const editPrice = ref<number | null>(null);
const editQuantity = ref<number | null>(null);
const confirming = ref<string | null>(null);
const tabsEl = ref<HTMLElement | null>(null);
const tabsCompact = useCompactTabs(tabsEl);

function confirmingAction(action: string, order: MyOrderDto) {
  return confirming.value === `${action}:${order.id}`;
}

const sellOrders = computed(() =>
  account.orders.filter((order) => order.kind === "sell"),
);
const buyOrders = computed(() =>
  account.orders.filter((order) => order.kind === "buy"),
);

// Own orders have no useful seller field, so the search covers the item and
// the variant the row already shows.
const shown = computed(() => {
  const search = query.value.trim().toLowerCase();
  const rows = tab.value === "sell" ? sellOrders.value : buyOrders.value;
  if (!search) return rows;
  return rows.filter((order) =>
    [order.item?.name ?? "", variantLabel(order)].some((text) =>
      text.toLowerCase().includes(search),
    ),
  );
});

const ownedBySlug = computed(() => {
  const counts = new Map<string, number>();
  for (const row of inventory.snapshot?.items ?? []) {
    const slug = row.market?.slug;
    if (!slug) continue;
    counts.set(slug, (counts.get(slug) ?? 0) + row.count);
  }
  return counts;
});

const ownedByRank = computed(() => {
  const counts = new Map<string, number>();
  for (const row of inventory.snapshot?.items ?? []) {
    const slug = row.market?.slug;
    if (!slug || row.rank == null) continue;
    const key = `${slug}|${row.rank}`;
    counts.set(key, (counts.get(key) ?? 0) + row.count);
  }
  return counts;
});

const tracked = computed(() => new Set(market.tracked));

const trackedValue = computed(() => {
  let total = 0;
  for (const row of inventory.snapshot?.items ?? []) {
    const slug = row.market?.slug;
    if (!slug || !tracked.value.has(slug)) continue;
    const quote = market.quotes[slug];
    const price =
      row.rank != null
        ? (quote?.variants.find((variant) => variant.rank === row.rank)?.platinum ?? null)
        : (quote?.platinum ?? null);
    if (price != null) total += price * row.count;
  }
  return total;
});

function capitalize(text: string) {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

function unitPrice(order: MyOrderDto) {
  const perTrade = order.per_trade ?? 1;
  return perTrade > 1 ? Math.floor(order.platinum / perTrade) : order.platinum;
}

function variantLabel(order: MyOrderDto) {
  const parts: string[] = [];
  if (order.rank != null) parts.push(formatRank(order.rank));
  if (order.amber_stars != null || order.cyan_stars != null) {
    parts.push(`${order.amber_stars ?? 0} amber / ${order.cyan_stars ?? 0} cyan`);
  }
  if (order.subtype) parts.push(capitalize(order.subtype));
  if (order.charges != null) parts.push(`${order.charges} charges`);
  if (order.per_trade != null) parts.push(`${order.per_trade}/trade`);
  return parts.join(" · ") || "Default";
}

function onHand(order: MyOrderDto): { text: string; warn: boolean } {
  if (!order.item) return { text: "unknown item", warn: false };
  const owned =
    order.rank != null
      ? (ownedByRank.value.get(`${order.item.slug}|${order.rank}`) ?? 0)
      : (ownedBySlug.value.get(order.item.slug) ?? 0);
  if (owned === 0) {
    return {
      text: order.rank != null ? `not owned at R${order.rank}` : "not owned",
      warn: true,
    };
  }
  if (owned < order.quantity) {
    return { text: `listed ${order.quantity} > owned ${owned}`, warn: true };
  }
  return { text: `${owned} owned`, warn: false };
}

function startEdit(order: MyOrderDto) {
  editingId.value = order.id;
  editPrice.value = unitPrice(order);
  editQuantity.value = order.quantity;
  confirming.value = null;
}

function cancelEdit() {
  editingId.value = null;
  editPrice.value = null;
  editQuantity.value = null;
}

async function saveEdit(order: MyOrderDto) {
  if (editPrice.value == null || editQuantity.value == null) return;
  const ok = await updateOrder(order, {
    platinum: editPrice.value * (order.per_trade ?? 1),
    quantity: editQuantity.value,
  });
  if (ok) cancelEdit();
}

async function toggleVisibility(order: MyOrderDto) {
  await setVisible(order, !order.visible);
}

// Resolve the menu before awaiting: event.currentTarget is cleared once the
// handler yields.
function menuOf(event: MouseEvent) {
  return (event.currentTarget as HTMLElement | null)?.closest("details") ?? null;
}

async function close(order: MyOrderDto, event: MouseEvent) {
  const menu = menuOf(event);
  await closeOrder(order, 1);
  menu?.removeAttribute("open");
}

async function markSold(order: MyOrderDto, event: MouseEvent) {
  const menu = menuOf(event);
  await closeOrder(order, Math.min(order.per_trade ?? 1, order.quantity));
  menu?.removeAttribute("open");
}

async function closeAll(order: MyOrderDto, event: MouseEvent) {
  if (!confirmingAction("close", order)) {
    confirming.value = `close:${order.id}`;
    return;
  }
  const menu = menuOf(event);
  confirming.value = null;
  await closeOrder(order, order.quantity);
  menu?.removeAttribute("open");
}

async function remove(order: MyOrderDto, event: MouseEvent) {
  if (!confirmingAction("delete", order)) {
    confirming.value = `delete:${order.id}`;
    return;
  }
  const menu = menuOf(event);
  confirming.value = null;
  await deleteOrder(order);
  menu?.removeAttribute("open");
}

function onGlobalRefresh() {
  void refreshOrders();
}

onMounted(async () => {
  window.addEventListener("coruframe:refresh", onGlobalRefresh);
  await Promise.all([loadAccount(), loadInventory(), initMarket()]);
  if (account.status.connected) await refreshOrders();
  await ensureQuotes(market.tracked);
});

onUnmounted(() => window.removeEventListener("coruframe:refresh", onGlobalRefresh));
</script>

<template>
  <div class="view">
    <PageBar>
      <template #actions>
        <SearchField v-model="query" placeholder="Search orders…" label="Search orders" />
        <span class="muted inventory-count">{{ shown.length }} orders</span>
      </template>

      <template #tabs>
        <div
          ref="tabsEl"
          class="inventory-tabs"
          :class="{ compact: tabsCompact }"
          role="tablist"
          aria-label="Order side"
        >
          <button
            type="button"
            role="tab"
            class="tier-tab"
            :class="{ active: tab === 'sell' }"
            :aria-selected="tab === 'sell'"
            aria-label="Sell orders"
            title="Sell orders"
            @click="tab = 'sell'"
          >
            <Icon name="arrowUp" :size="16" />
            <span class="tab-label">Sell</span>
            <span class="tab-count">{{ sellOrders.length }}</span>
          </button>
          <button
            type="button"
            role="tab"
            class="tier-tab"
            :class="{ active: tab === 'buy' }"
            :aria-selected="tab === 'buy'"
            aria-label="Buy orders"
            title="Buy orders"
            @click="tab = 'buy'"
          >
            <Icon name="arrowDown" :size="16" />
            <span class="tab-label">Buy</span>
            <span class="tab-count">{{ buyOrders.length }}</span>
          </button>
        </div>
      </template>
    </PageBar>

    <div class="content">
      <PageHeader
        title="Trading"
        description="Sell orders and tracked value."
        icon="trading"
      />

      <div class="summary-row">
        <div class="summary-card tone-green">
          <span class="summary-icon"><Icon name="shop" :size="18" /></span>
          <div><strong>{{ openSellCount() }}</strong><span>Visible sell orders</span></div>
        </div>
        <div class="summary-card tone-gold">
          <span class="summary-icon"><Icon name="trading" :size="18" /></span>
          <div><strong>{{ formatPlatinum(listedValue()) }}</strong><span>Listed platinum</span></div>
        </div>
        <div class="summary-card tone-blue">
          <span class="summary-icon"><Icon name="inventory" :size="18" /></span>
          <div>
            <strong>{{ formatPlatinum(trackedValue) }}</strong>
            <span>Tracked inventory value</span>
          </div>
        </div>
        <div class="summary-card tone-blue">
          <span class="summary-icon"><Icon name="star" :size="18" /></span>
          <div><strong>{{ market.tracked.length }}</strong><span>Tracked items</span></div>
        </div>
      </div>

      <div v-if="account.error" class="market-error">{{ account.error }}</div>
      <div v-if="account.status.last_error" class="warning-banner">
        <Icon name="shield" :size="16" />
        <div>{{ account.status.last_error }}</div>
      </div>
      <div v-if="!account.status.connected" class="warning-banner">
        <Icon name="lock" :size="16" />
        <div>
          <strong>Not signed in to warframe.market.</strong>
          <p>Sign in from Settings to manage orders.</p>
          <RouterLink to="/settings" class="button ghost small">Open settings</RouterLink>
        </div>
      </div>

      <div class="panel">
        <p v-if="tab === 'buy'" class="readonly-note">
          <Icon name="lock" :size="13" />
          Buy orders are read-only here.
        </p>

        <div v-if="shown.length" class="data-table-wrap">
          <table class="data-table">
            <thead>
              <tr>
                <th>Item</th>
                <th>Variant</th>
                <th class="num">Price/unit</th>
                <th class="num">Qty</th>
                <th>Visibility</th>
                <th>On hand</th>
                <th class="num">Actions</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="order in shown" :key="order.id">
                <td class="strong">
                  {{ order.item?.name ?? "Unknown item" }}
                </td>
                <td class="muted">{{ variantLabel(order) }}</td>
                <td
                  class="num"
                  :title="
                    (order.per_trade ?? 1) > 1
                      ? `${order.per_trade}/trade · ${formatPlatinum(order.platinum)}/trade`
                      : undefined
                  "
                >
                  <input
                    v-if="editingId === order.id"
                    v-model.number="editPrice"
                    type="number"
                    min="1"
                    max="900000"
                    class="inline-number"
                    aria-label="Price"
                  />
                  <template v-else>{{ formatPlatinum(unitPrice(order)) }}</template>
                </td>
                <td class="num">
                  <input
                    v-if="editingId === order.id"
                    v-model.number="editQuantity"
                    type="number"
                    min="1"
                    max="9999"
                    class="inline-number"
                    aria-label="Quantity"
                  />
                  <template v-else>{{ formatCount(order.quantity) }}</template>
                </td>
                <td>
                  <span class="badge" :class="order.visible ? 'owned' : 'vaulted'">
                    {{ order.visible ? "Visible" : "Hidden" }}
                  </span>
                </td>
                <td>
                  <span :class="{ muted: !onHand(order).warn, 'on-hand-warn': onHand(order).warn }">
                    {{ onHand(order).text }}
                  </span>
                </td>
                <td class="num order-actions">
                  <span v-if="tab === 'buy'" class="muted">Read-only</span>
                  <template v-else-if="editingId === order.id">
                    <button class="button ghost small" type="button" @click="saveEdit(order)">
                      Save
                    </button>
                    <button class="button ghost small" type="button" @click="cancelEdit">
                      Cancel
                    </button>
                  </template>
                  <template v-else>
                    <button class="button ghost small" type="button" @click="startEdit(order)">
                      Edit
                    </button>
                    <button class="button ghost small" type="button" @click="toggleVisibility(order)">
                      {{ order.visible ? "Hide" : "Show" }}
                    </button>
                    <details class="row-menu">
                      <summary class="button ghost small">More</summary>
                      <div class="row-menu-items">
                        <button class="button ghost small" type="button" @click="close(order, $event)">
                          Close 1
                        </button>
                        <button
                          class="button ghost small"
                          type="button"
                          :title="`Close one trade (${order.per_trade ?? 1} units)`"
                          @click="markSold(order, $event)"
                        >
                          Mark sold
                        </button>
                        <button class="button ghost small" type="button" @click="closeAll(order, $event)">
                          {{ confirmingAction("close", order) ? "Confirm close all" : "Close all" }}
                        </button>
                        <button class="button danger small" type="button" @click="remove(order, $event)">
                          {{ confirmingAction("delete", order) ? "Confirm delete" : "Delete" }}
                        </button>
                      </div>
                    </details>
                  </template>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
        <div v-else class="empty-state">
          <Icon :name="tab === 'sell' ? 'shop' : 'arrowDown'" :size="30" />
          <h3>
            {{
              !account.status.connected
                ? "Sign in to see your orders"
                : tab === "sell"
                  ? "No sell orders"
                  : "No buy orders"
            }}
          </h3>
          <p v-if="!account.status.connected">
            Sign in from Settings to manage orders.
          </p>
          <p v-else-if="query">
            No {{ tab }} order matches “{{ query }}”.
          </p>
          <p v-else-if="tab === 'sell'">Use the Inventory page to list an item.</p>
          <p v-else>Buy orders you place on warframe.market appear here.</p>
          <RouterLink
            v-if="!account.status.connected"
            to="/settings"
            class="button ghost"
          >
            Open settings
          </RouterLink>
        </div>
      </div>

    </div>
  </div>
</template>
