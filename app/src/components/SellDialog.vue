<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import Icon from "./Icon.vue";
import Toggle from "./Toggle.vue";
import { focusItem, market } from "../stores/market";
import {
  account,
  createOrder,
  loadAccount,
  refreshOrders,
  updateOrder,
} from "../stores/account";
import { inventory, type SellTarget } from "../stores/inventory";
import { openExternal } from "../lib/api";
import { formatCount, formatPlatinum, formatRank } from "../lib/format";
import type { OrderDto } from "../lib/api";

const props = defineProps<{ target: SellTarget }>();
const emit = defineEmits<{ close: [] }>();
const router = useRouter();

const quantity = ref(1);
const price = ref<number | null>(null);
const visible = ref(true);
const rank = ref<number | null>(null);
const charges = ref<number | null>(null);
const amber = ref<number | null>(null);
const cyan = ref<number | null>(null);
const subtype = ref<string | null>(null);
const perTrade = ref(1);
const priceTouched = ref(false);
const noOwnedRank = ref(false);
const busy = ref(false);
const result = ref("");
const failure = ref("");
const copiedKey = ref("");
const listingCopied = ref(false);
const dialog = ref<HTMLElement | null>(null);
const closeButton = ref<HTMLButtonElement | null>(null);
let opener: HTMLElement | null = null;
let ready = false;
let lastFilter = "";

const item = computed(() => props.target.market);
const focus = computed(() =>
  market.focus?.item.slug === item.value.slug ? market.focus : null,
);
const quote = computed(
  () => focus.value?.quote ?? market.quotes[item.value.slug] ?? null,
);
// The focus payload carries every online order; a listing dialog must only
// compare against the variant being sold, or an R3 seller sets the R0 price.
function matchesVariant(order: OrderDto) {
  return (
    (rank.value == null || order.rank === rank.value) &&
    (charges.value == null || order.charges === charges.value) &&
    (amber.value == null || order.amber_stars === amber.value) &&
    (cyan.value == null || order.cyan_stars === cyan.value) &&
    (subtype.value == null || order.subtype === subtype.value)
  );
}
const sells = computed(() =>
  (focus.value?.sell ?? []).filter(matchesVariant).slice(0, 3),
);
const buys = computed(() => (focus.value?.buy ?? []).filter(matchesVariant));
const bestBuy = computed(() => buys.value[0] ?? null);
const lowestOrder = computed(() => sells.value[0] ?? null);
const lowest = computed(() => lowestOrder.value?.unit_price ?? null);
const suggested = computed(() => (lowest.value ? Math.max(1, lowest.value - 1) : null));
const tradeTotal = computed(() =>
  price.value != null ? price.value * perTrade.value : null,
);
const crossed = computed(
  () =>
    price.value != null &&
    bestBuy.value != null &&
    price.value <= bestBuy.value.unit_price,
);

const chargeOptions = computed(() =>
  item.value.max_charges != null
    ? Array.from({ length: item.value.max_charges + 1 }, (_, index) => index)
    : [],
);
const rankOptions = computed(() =>
  item.value.max_rank != null
    ? Array.from({ length: item.value.max_rank + 1 }, (_, index) => index)
    : [],
);
const amberOptions = computed(() =>
  item.value.max_amber_stars != null
    ? Array.from({ length: item.value.max_amber_stars + 1 }, (_, index) => index)
    : [],
);
const cyanOptions = computed(() =>
  item.value.max_cyan_stars != null
    ? Array.from({ length: item.value.max_cyan_stars + 1 }, (_, index) => index)
    : [],
);
const subtypes = computed(() => item.value.subtypes);
// A relic row is one owned refinement, so selling it may not switch subtypes.
const subtypeLocked = computed(
  () =>
    props.target.subtype != null && subtypes.value.includes(props.target.subtype),
);
const ranked = computed(() => item.value.max_rank != null);
// A stack size must divide the quantity. Offer the largest valid stack first;
// WFM rejects a perTrade that does not divide the order quantity.
const perTradeOptions = computed(() => {
  const options: number[] = [];
  for (let size = Math.min(6, quantity.value); size >= 1; size -= 1) {
    if (quantity.value % size === 0) options.push(size);
  }
  return options;
});
const hasVariantControls = computed(
  () =>
    chargeOptions.value.length > 1 ||
    amberOptions.value.length > 1 ||
    cyanOptions.value.length > 1 ||
    subtypes.value.length > 1,
);
const connected = computed(() => account.status.connected);

function highestOwnedRank(slug: string): number | null {
  const rows = inventory.snapshot?.items ?? [];
  const ranks = rows
    .filter((row) => row.market?.slug === slug && row.rank != null)
    .map((row) => row.rank as number);
  return ranks.length ? Math.max(...ranks) : null;
}

function currentFilter() {
  return {
    rank: rank.value,
    charges: charges.value,
    amber_stars: amber.value,
    cyan_stars: cyan.value,
    subtype: subtype.value,
  };
}

function syncFocus() {
  if (!ready) return;
  const key = JSON.stringify(currentFilter());
  if (key === lastFilter) return;
  lastFilter = key;
  void focusItem(item.value, currentFilter());
}

const duplicate = computed(() =>
  account.orders.find(
    (order) =>
      order.kind === "sell" &&
      order.item_id === item.value.id &&
      order.rank === rank.value &&
      order.charges === charges.value &&
      order.amber_stars === amber.value &&
      order.cyan_stars === cyan.value &&
      order.subtype === subtype.value,
  ),
);

const variantLabel = computed(() => {
  const parts: string[] = [];
  if (rank.value != null) parts.push(formatRank(rank.value));
  if (amber.value != null || cyan.value != null) {
    parts.push(`${amber.value ?? 0}/${cyan.value ?? 0} stars`);
  }
  if (subtype.value) {
    parts.push(subtype.value.charAt(0).toUpperCase() + subtype.value.slice(1));
  }
  if (charges.value != null) parts.push(`${charges.value} charges`);
  if (perTrade.value > 1) {
    parts.push(`${perTrade.value}/trade`);
  }
  return parts.join(" · ") || "Default";
});

const validation = computed(() => {
  if (price.value == null || price.value < 1 || price.value > 900_000) {
    return "Price must be between 1 and 900000 platinum.";
  }
  if (quantity.value < 1 || quantity.value > 9_999) {
    return "Quantity must be between 1 and 9999.";
  }
  if (props.target.count > 0 && quantity.value > props.target.count) {
    return `Quantity exceeds the ${props.target.count} you own.`;
  }
  return "";
});
const canSave = computed(
  () => connected.value && !busy.value && !validation.value && price.value != null,
);

function attrs(order: OrderDto) {
  const parts: { key: string; icon?: "zap" | "amberStar" | "cyanStar"; text: string }[] = [];
  if (order.rank != null) parts.push({ key: "rank", text: formatRank(order.rank) });
  if (order.charges != null) parts.push({ key: "charges", icon: "zap", text: String(order.charges) });
  if (order.amber_stars != null) {
    parts.push({ key: "amber", icon: "amberStar", text: String(order.amber_stars) });
  }
  if (order.cyan_stars != null) {
    parts.push({ key: "cyan", icon: "cyanStar", text: String(order.cyan_stars) });
  }
  if (order.subtype) parts.push({ key: "subtype", text: order.subtype });
  if (order.per_trade > 1) {
    parts.push({
      key: "trade",
      text: `${order.per_trade}/trade · ${formatPlatinum(order.platinum)}/trade`,
    });
  }
  return parts;
}

function key(order: OrderDto, side: string) {
  return `${side}-${order.ingame_name}-${order.platinum}`;
}

function copyWhisper(order: OrderDto, side: "buy" | "sell") {
  const verb = side === "buy" ? "buy" : "sell";
  const text = `/w ${order.ingame_name} Hi! I want to ${verb}: "${props.target.name}" for ${order.unit_price} platinum. (warframe.market via CoruFrame)`;
  void navigator.clipboard?.writeText(text).then(() => {
    copiedKey.value = key(order, side);
    window.setTimeout(() => (copiedKey.value = ""), 1600);
  });
}

function copyListing() {
  if (price.value == null) return;
  const variant = ranked.value || hasVariantControls.value ? ` (${variantLabel.value})` : "";
  const text = `Listing: ${props.target.name}${variant} ×${quantity.value} at ${price.value}p/unit (warframe.market via CoruFrame)`;
  void navigator.clipboard?.writeText(text).then(() => {
    listingCopied.value = true;
    window.setTimeout(() => (listingCopied.value = false), 1600);
  });
}

function openMarket() {
  void openExternal(`https://warframe.market/items/${item.value.slug}`);
}

function goSettings() {
  emit("close");
  void router.push("/settings");
}

function input() {
  return {
    item_id: item.value.id,
    platinum: (price.value ?? 0) * perTrade.value,
    quantity: quantity.value,
    visible: visible.value,
    rank: rank.value,
    charges: charges.value,
    amber_stars: amber.value,
    cyan_stars: cyan.value,
    subtype: subtype.value,
    per_trade: perTrade.value > 1 ? perTrade.value : null,
  };
}

async function save() {
  if (!canSave.value) return;
  busy.value = true;
  result.value = "";
  failure.value = "";
  const existing = duplicate.value;
  const values = input();
  const ok = existing
    ? await updateOrder(existing, {
        platinum: values.platinum,
        quantity: values.quantity,
        visible: values.visible,
        rank: values.rank,
        charges: values.charges,
        amber_stars: values.amber_stars,
        cyan_stars: values.cyan_stars,
        subtype: values.subtype,
        per_trade: values.per_trade,
      })
    : await createOrder(values);
  busy.value = false;
  if (ok) {
    result.value = existing
      ? `Order ${existing.id} updated.`
      : `Order ${account.lastOrderId ?? ""} created.`.replace("  ", " ");
  } else {
    failure.value = account.error || "The order was not saved.";
  }
}

watch(suggested, (value) => {
  if (!priceTouched.value && value != null) price.value = value;
});

watch([rank, charges, amber, cyan, subtype], syncFocus);
watch(quantity, () => {
  if (!perTradeOptions.value.includes(perTrade.value)) {
    perTrade.value = perTradeOptions.value[0] ?? 1;
  }
});

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    emit("close");
    return;
  }
  if (event.key !== "Tab") return;
  const root = dialog.value;
  if (!root) return;
  const focusable = root.querySelectorAll<HTMLElement>(
    'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
  );
  if (!focusable.length) {
    event.preventDefault();
    root.focus();
    return;
  }
  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  const active = document.activeElement;
  if (event.shiftKey && (active === first || !root.contains(active))) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && (active === last || !root.contains(active))) {
    event.preventDefault();
    first.focus();
  }
}

onMounted(async () => {
  opener = document.activeElement as HTMLElement | null;
  document.body.classList.add("dialog-open");
  const owned = props.target.rank ?? highestOwnedRank(item.value.slug);
  noOwnedRank.value = owned == null && item.value.max_rank != null;
  rank.value = owned ?? item.value.max_rank;
  charges.value = item.value.max_charges;
  amber.value = item.value.max_amber_stars;
  cyan.value = item.value.max_cyan_stars;
  subtype.value = props.target.subtype ?? item.value.subtypes[0] ?? null;
  quantity.value = Math.max(1, Math.min(props.target.count || 1, 9_999));
  perTrade.value = perTradeOptions.value[0] ?? 1;
  await nextTick();
  closeButton.value?.focus();
  ready = true;
  lastFilter = JSON.stringify(currentFilter());
  void loadAccount().then(() => {
    if (account.status.connected) void refreshOrders();
  });
  void focusItem(item.value, currentFilter());
});

onBeforeUnmount(() => {
  document.body.classList.remove("dialog-open");
  if (opener?.isConnected) opener.focus();
});
</script>

<template>
  <div class="dialog-backdrop" @click.self="emit('close')">
    <section
      ref="dialog"
      class="sell-dialog"
      role="dialog"
      aria-modal="true"
      :aria-label="`Sell ${target.name}`"
      tabindex="-1"
      @keydown="onKeydown"
    >
      <header class="dialog-head">
        <div class="dialog-title">
          <div class="eyebrow">{{ duplicate ? "Update sell order" : "List on warframe.market" }}</div>
          <h2>{{ target.name }}</h2>
          <span class="muted">
            Owned ×{{ target.count }} · {{ target.market.vaulted ? "Vaulted" : "Current" }}
            <template v-if="rank != null"> · R{{ rank }}</template>
          </span>
        </div>
        <button
          ref="closeButton"
          class="icon-button small"
          type="button"
          aria-label="Close"
          @click="emit('close')"
        >
          <Icon name="x" :size="14" />
        </button>
      </header>

      <div class="sell-summary">
        <div class="sell-price">
          <span>Lowest sell</span>
          <strong class="selectable">{{ formatPlatinum(lowest) }}<small>/unit</small></strong>
          <span v-if="lowestOrder && lowestOrder.per_trade > 1" class="sell-trade">
            {{ lowestOrder.per_trade }}/trade ·
            {{ formatPlatinum(lowestOrder.platinum) }}/trade
          </span>
        </div>
        <div class="sell-price">
          <span>Best buy</span>
          <strong class="selectable">{{ formatPlatinum(bestBuy?.unit_price) }}<small>/unit</small></strong>
          <span v-if="bestBuy && bestBuy.per_trade > 1" class="sell-trade">
            {{ bestBuy.per_trade }}/trade · {{ formatPlatinum(bestBuy.platinum) }}/trade
          </span>
        </div>
        <div class="sell-price suggested">
          <span>Suggested</span>
          <strong class="selectable">{{ formatPlatinum(suggested) }}<small>/unit</small></strong>
          <span v-if="perTrade > 1 && suggested" class="sell-trade">
            {{ perTrade }}/trade · {{ formatPlatinum(suggested * perTrade) }}/trade
          </span>
        </div>
      </div>

      <div v-if="crossed" class="warning-banner compact">
        <Icon name="shield" :size="15" />
        <div>Price is at or below the best buy order.</div>
      </div>
      <div v-if="!connected" class="warning-banner compact">
        <Icon name="lock" :size="15" />
        <div>Connect a warframe.market account in Settings to list orders.</div>
      </div>
      <div v-if="market.error" class="market-error">{{ market.error }}</div>
      <div v-if="failure" class="market-error">{{ failure }}</div>
      <div v-if="result" class="warning-banner compact">
        <Icon name="check" :size="15" />
        <div>{{ result }}</div>
      </div>

      <div v-if="ranked && rank != null" class="sell-rank">
        Selling at <strong>R{{ rank }}</strong> · owned ×{{ target.count }}
      </div>
      <div v-if="noOwnedRank" class="warning-banner compact">
        <Icon name="shield" :size="15" />
        <div>No owned copy at any rank; defaulting to R{{ rank }}.</div>
      </div>

      <div
        v-if="hasVariantControls"
        class="order-filters sell-filters"
      >
        <label v-if="chargeOptions.length" class="order-filter">
          <span>Charges</span>
          <select v-model.number="charges">
            <option v-for="value in chargeOptions" :key="value" :value="value">{{ value }}</option>
          </select>
        </label>
        <label v-if="amberOptions.length" class="order-filter">
          <span>Amber</span>
          <select v-model.number="amber">
            <option v-for="value in amberOptions" :key="value" :value="value">{{ value }}</option>
          </select>
        </label>
        <label v-if="cyanOptions.length" class="order-filter">
          <span>Cyan</span>
          <select v-model.number="cyan">
            <option v-for="value in cyanOptions" :key="value" :value="value">{{ value }}</option>
          </select>
        </label>
        <label v-if="subtypes.length" class="order-filter">
          <span>Subtype</span>
          <select v-model="subtype" :disabled="subtypeLocked">
            <option v-for="value in subtypes" :key="value" :value="value">
              {{ value.charAt(0).toUpperCase() + value.slice(1) }}
            </option>
          </select>
        </label>
      </div>
      <p v-if="ranked" class="dialog-note">
        The rank picks the exact variant to list, so this creates an R{{ rank }} order.
      </p>

      <div class="sell-controls">
        <label class="field quantity-field">
          <span>Price per unit</span>
          <input
            v-model.number="price"
            type="number"
            min="1"
            max="900000"
            @input="priceTouched = true"
          />
          <small v-if="perTrade > 1 && tradeTotal != null" class="field-note">
            {{ perTrade }} per trade · {{ formatPlatinum(tradeTotal) }}/trade
          </small>
        </label>
        <label class="field quantity-field">
          <span>Quantity</span>
          <input v-model.number="quantity" type="number" min="1" :max="target.count" />
        </label>
        <label v-if="rankOptions.length" class="field quantity-field">
          <span>Rank</span>
          <select v-model.number="rank">
            <option v-for="value in rankOptions" :key="value" :value="value">R{{ value }}</option>
          </select>
        </label>
        <label v-if="perTradeOptions.length > 1" class="field quantity-field">
          <span>Per trade</span>
          <select v-model.number="perTrade">
            <option v-for="value in perTradeOptions" :key="value" :value="value">{{ value }}</option>
          </select>
        </label>
      </div>
      <Toggle
        v-model="visible"
        label="Visible"
        description="Hidden orders stay on the site but do not appear in search."
      />

      <div class="sell-confirm">
        <div><span>Item</span><strong>{{ target.name }}</strong></div>
        <div><span>Variant</span><strong>{{ variantLabel }}</strong></div>
        <div><span>Price/unit</span><strong>{{ formatPlatinum(price) }}</strong></div>
        <div v-if="perTrade > 1 && tradeTotal != null">
          <span>Per trade</span><strong>{{ perTrade }} × {{ formatPlatinum(price) }} = {{ formatPlatinum(tradeTotal) }}</strong>
        </div>
        <div><span>Quantity</span><strong>{{ formatCount(quantity) }}</strong></div>
        <div><span>Visible</span><strong>{{ visible ? "Yes" : "No" }}</strong></div>
      </div>
      <p v-if="validation" class="market-error">{{ validation }}</p>
      <p v-if="duplicate" class="dialog-note">
        This variant is already listed as order <code>{{ duplicate.id }}</code>; saving updates
        it instead of creating a duplicate.
      </p>

      <div class="sell-controls actions">
        <button class="button ghost" type="button" :disabled="price == null" @click="copyListing">
          <Icon name="copy" :size="14" />
          {{ listingCopied ? "Copied" : "Copy listing" }}
        </button>
        <button v-if="!connected" class="button primary" type="button" @click="goSettings">
          <Icon name="settings" :size="14" /> Connect in Settings
        </button>
        <button
          v-else
          class="button primary"
          type="button"
          :disabled="!canSave"
          @click="save"
        >
          <Icon name="shop" :size="14" />
          {{ busy ? "Saving…" : duplicate ? "Update order" : "List order" }}
        </button>
      </div>

      <div class="rail-section-head">
        <h2>Lowest sell orders</h2>
        <span class="muted">{{ quote ? `${quote.online} online` : market.focusLoading ? "loading" : "—" }}</span>
      </div>
      <div class="market-orders sell-orders">
        <div
          v-for="order in sells"
          :key="key(order, 'buy')"
          class="market-order"
        >
          <span class="market-order-status" :class="order.status"></span>
          <div class="market-order-main">
            <span class="market-order-name selectable">{{ order.ingame_name }}</span>
            <span v-if="attrs(order).length" class="market-order-attrs">
              <template v-for="attr in attrs(order)" :key="attr.key">
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
            :class="{ copied: copiedKey === key(order, 'buy') }"
            type="button"
            :aria-label="`Copy whisper to ${order.ingame_name}`"
            @click="copyWhisper(order, 'buy')"
          >
            <Icon :name="copiedKey === key(order, 'buy') ? 'check' : 'copy'" :size="12" />
          </button>
        </div>
        <div v-if="!sells.length" class="market-empty">
          {{ market.focusLoading ? "Loading orders…" : "No online sell orders." }}
        </div>
      </div>

      <div class="rail-section-head"><h2>Best buy order</h2></div>
      <div v-if="bestBuy" class="market-order buy">
        <span class="market-order-status" :class="bestBuy.status"></span>
        <div class="market-order-main">
          <span class="market-order-name selectable">{{ bestBuy.ingame_name }}</span>
          <span v-if="attrs(bestBuy).length" class="market-order-attrs">
            <template v-for="attr in attrs(bestBuy)" :key="attr.key">
              <span v-if="attr.icon" class="order-attr">
                <Icon :name="attr.icon" :size="10" :class="`order-attr-${attr.key}`" />{{ attr.text }}
              </span>
              <span v-else class="order-attr">{{ attr.text }}</span>
            </template>
          </span>
        </div>
        <span class="market-order-qty">{{ formatCount(bestBuy.quantity) }}</span>
        <span class="market-order-price selectable">{{ formatPlatinum(bestBuy.unit_price) }}</span>
        <button
          class="market-order-copy"
          :class="{ copied: copiedKey === key(bestBuy, 'sell') }"
          type="button"
          :aria-label="`Copy whisper to ${bestBuy.ingame_name}`"
          @click="copyWhisper(bestBuy, 'sell')"
        >
          <Icon :name="copiedKey === key(bestBuy, 'sell') ? 'check' : 'copy'" :size="12" />
        </button>
      </div>
      <div v-else class="market-empty">No online buy order.</div>

      <footer class="dialog-actions">
        <button class="button ghost" type="button" @click="openMarket">
          <Icon name="external" :size="14" /> Open market
        </button>
        <button class="button ghost" type="button" @click="emit('close')">Close</button>
      </footer>
    </section>
  </div>
</template>
