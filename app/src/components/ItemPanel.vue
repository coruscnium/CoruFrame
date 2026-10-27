<script setup lang="ts">
import { computed } from "vue";
import Icon from "./Icon.vue";
import ItemArt from "./ItemArt.vue";
import { app, clearSelection } from "../stores/app";
import { inventory } from "../stores/inventory";
import { focusItem, focusMarketSlug, iconUrl } from "../stores/market";

const selected = computed(() => app.selected);
const snapshot = computed(() => inventory.snapshot);
const masteryAvailable = computed(() => snapshot.value?.mastery_available === true);

const parts = computed(() =>
  (selected.value?.components ?? []).map((part) => ({
    ...part,
    have: snapshot.value?.component_counts[part.unique_name] ?? 0,
  })),
);

const owned = computed(() => {
  const card = selected.value;
  if (!card) return false;
  if (card.owned != null) return card.owned;
  return (snapshot.value?.items ?? []).some(
    (row) => row.resolved && row.item.unique_name === card.item.unique_name,
  );
});

const mastered = computed(() => {
  const card = selected.value;
  if (!card) return false;
  if (card.mastered != null) return card.mastered;
  return (snapshot.value?.mastery.mastered ?? []).includes(card.item.unique_name);
});

const marketSet = computed(() => selected.value?.market ?? null);
const tradable = computed(
  () => Boolean(marketSet.value || parts.value.some((part) => part.market_slug)),
);
const setAction = computed(() => {
  const market = marketSet.value;
  if (!market) return "";
  return market.name.toLowerCase().includes("set") ? "Whole set on market" : "Market";
});

function openSet() {
  const market = marketSet.value;
  if (!market) return;
  app.railTab = "market";
  void focusItem(market, app.selectedFilter ?? undefined);
}

function openPart(slug: string) {
  void focusMarketSlug(slug);
}
</script>

<template>
  <div v-if="!selected" class="rail-panel item-panel item-panel-empty">
    <Icon name="inventory" :size="28" />
    <h2>No item selected</h2>
    <p class="muted">Select an item to see parts and prices.</p>
  </div>
  <div v-else class="rail-panel item-panel">
    <div class="item-detail-head">
      <span class="item-detail-art">
        <ItemArt
          :image="selected.item.image_url"
          :market-icon="
            selected.market ? iconUrl(selected.market.thumb ?? selected.market.icon) : null
          "
          :kind="selected.item.kind"
          :icon-size="22"
          alt=""
        />
      </span>
      <div class="item-detail-title">
        <h2 class="selectable">{{ selected.item.name }}</h2>
        <div class="market-focus-meta">
          <span class="badge" :class="owned ? 'owned' : 'neutral'">
            {{ owned ? "Owned" : "Not owned" }}
          </span>
          <span v-if="masteryAvailable" class="badge neutral">
            {{ mastered ? "Mastered" : "Not mastered" }}
          </span>
          <span class="badge neutral">{{ tradable ? "Tradable" : "Not tradable" }}</span>
          <span v-if="selected.item.vaulted" class="badge vaulted">
            <Icon name="lock" :size="10" /> Vaulted
          </span>
          <span v-else-if="selected.item.prime" class="badge neutral">Prime</span>
          <span v-if="selected.item.mastery_req" class="badge neutral">
            MR {{ selected.item.mastery_req }}
          </span>
        </div>
      </div>
      <button
        class="icon-button small"
        type="button"
        aria-label="Close item details"
        @click="clearSelection"
      >
        <Icon name="x" :size="14" />
      </button>
    </div>

    <div v-if="parts.length" class="rail-section-head">
      <h2>Components</h2>
      <span class="muted">owned / needed</span>
    </div>
    <div v-if="parts.length" class="item-part-list">
      <div v-for="part in parts" :key="part.unique_name" class="item-part-row">
        <span class="item-part-name">{{ part.name }}</span>
        <button
          v-if="part.market_slug"
          class="button ghost small item-part-market"
          type="button"
          @click="openPart(part.market_slug!)"
        >
          <Icon name="external" :size="12" /> Market
        </button>
        <span class="item-part-count" :class="{ complete: part.have >= part.count }">
          {{ part.have }} / {{ part.count }}
        </span>
      </div>
    </div>
    <p v-if="parts.length && !parts.some((part) => part.market_slug)" class="muted item-panel-note">
      None of these parts are tradable.
    </p>
    <p v-else-if="!parts.length && !marketSet" class="muted item-panel-note">
      No parts or market listings for this item.
    </p>

    <button v-if="marketSet" class="button primary item-set-action" type="button" @click="openSet">
      <Icon name="external" :size="13" /> {{ setAction }}
    </button>
  </div>
</template>
