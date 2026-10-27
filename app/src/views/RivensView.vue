<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import Icon from "../components/Icon.vue";
import PageHeader from "../components/PageHeader.vue";
import PageBar from "../components/PageBar.vue";
import SearchField from "../components/SearchField.vue";
import WfIcon from "../components/WfIcon.vue";
import {
  isTauri,
  openExternal,
  rivenInventory,
  rivenListings,
  type RivenDto,
  type RivenListingDto,
} from "../lib/api";
import { formatAge, formatPlatinum, formatRank } from "../lib/format";
import { useCompactTabs } from "../lib/useCompactTabs";

const owned = ref<RivenDto[]>([]);
const listings = ref<RivenListingDto[]>([]);
const loading = ref(true);
const ownedError = ref("");
const listingError = ref("");

const unveiled = computed(() => owned.value.filter((riven) => !riven.veiled).length);
const veiled = computed(() => owned.value.length - unveiled.value);
const section = ref<"owned" | "listings">("owned");
const tabsEl = ref<HTMLElement | null>(null);
const tabsCompact = useCompactTabs(tabsEl);
const query = ref("");
const filteredOwned = computed(() => {
  const terms = query.value.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (!terms.length) return owned.value;
  return owned.value.filter((riven) => {
    const text = [
      riven.name,
      riven.weapon,
      riven.weapon_type,
      riven.polarity,
      riven.challenge,
      ...riven.stats.map((stat) => stat.name),
    ]
      .filter(Boolean)
      .join(" ")
      .toLowerCase();
    return terms.every((term) => text.includes(term));
  });
});
const listedValue = computed(() =>
  listings.value.reduce(
    (sum, listing) => sum + (listing.buyout_price ?? listing.top_bid ?? listing.starting_price),
    0,
  ),
);

function message(reason: unknown) {
  if (typeof reason === "string") return reason;
  if (reason instanceof Error) return reason.message;
  return String(reason);
}

async function refresh() {
  loading.value = true;
  ownedError.value = "";
  listingError.value = "";
  const [rivens, auctions] = await Promise.allSettled([rivenInventory(), rivenListings()]);
  if (rivens.status === "fulfilled") {
    owned.value = rivens.value;
  } else {
    ownedError.value = `Inventory rivens unavailable: ${message(rivens.reason)}`;
  }
  if (auctions.status === "fulfilled") {
    listings.value = auctions.value;
  } else {
    listingError.value = message(auctions.reason);
  }
  loading.value = false;
}

function format(value: number) {
  return Number.isInteger(value) ? String(value) : value.toFixed(1);
}

function statText(stat: { value: number; unit: string; positive: boolean }) {
  return `${stat.positive ? "+" : "−"}${format(stat.value)}${stat.unit}`;
}

function listingPrice(listing: RivenListingDto) {
  const price = listing.buyout_price ?? listing.top_bid ?? listing.starting_price;
  return formatPlatinum(price);
}

function listedAgo(created: string) {
  const at = Date.parse(created);
  if (Number.isNaN(at)) return "listed";
  return `listed ${formatAge(Math.floor((Date.now() - at) / 1000))}`;
}

function openAuctions() {
  void openExternal("https://warframe.market/auctions");
}

function onGlobalRefresh() {
  void refresh();
}

onMounted(() => {
  void refresh();
  window.addEventListener("coruframe:refresh", onGlobalRefresh);
});

onUnmounted(() => window.removeEventListener("coruframe:refresh", onGlobalRefresh));
</script>

<template>
  <div class="view">
    <PageBar>
      <template #actions>
        <SearchField
          v-if="section === 'owned'"
          v-model="query"
          placeholder="Search weapon, riven, stat…"
          label="Search owned rivens"
        />
        <span v-if="!isTauri()" class="preview-chip warn">
          <Icon name="lock" :size="13" /> Preview data
        </span>
        <button class="button" type="button" @click="openAuctions">
          <Icon name="trading" :size="14" /> Auctions
        </button>
      </template>

      <template #tabs>
        <div
          ref="tabsEl"
          class="inventory-tabs"
          :class="{ compact: tabsCompact }"
          role="tablist"
          aria-label="Rivens section"
        >
          <button
            type="button"
            role="tab"
            class="tier-tab"
            :class="{ active: section === 'owned' }"
            :aria-selected="section === 'owned'"
            aria-label="Owned rivens"
            title="Owned rivens"
            @click="section = 'owned'"
          >
            <WfIcon name="rivens" :size="16" />
            <span class="tab-label">Owned</span>
            <span class="tab-count">{{ owned.length }}</span>
          </button>
          <button
            type="button"
            role="tab"
            class="tier-tab"
            :class="{ active: section === 'listings' }"
            :aria-selected="section === 'listings'"
            aria-label="Listings"
            title="Listings"
            @click="section = 'listings'"
          >
            <WfIcon name="trading" :size="16" />
            <span class="tab-label">Listings</span>
            <span class="tab-count">{{ listings.length }}</span>
          </button>
        </div>
      </template>
    </PageBar>

    <div class="content">
      <PageHeader
        title="Rivens"
        description="Owned rivens, listings and auctions."
        icon="rivens"
      />

      <div class="summary-row">
        <div class="summary-card tone-blue">
          <span class="summary-icon"><Icon name="rivens" :size="18" /></span>
          <div><strong>{{ owned.length }}</strong><span>Rivens owned</span></div>
        </div>
        <div class="summary-card tone-green">
          <span class="summary-icon"><Icon name="sparkles" :size="18" /></span>
          <div><strong>{{ unveiled }}</strong><span>Unveiled</span></div>
        </div>
        <div class="summary-card tone-red">
          <span class="summary-icon"><Icon name="lock" :size="18" /></span>
          <div><strong>{{ veiled }}</strong><span>Veiled</span></div>
        </div>
        <div class="summary-card tone-gold">
          <span class="summary-icon"><Icon name="trading" :size="18" /></span>
          <div>
            <strong>{{ formatPlatinum(listedValue) }}</strong>
            <span>{{ listings.length }} active listing{{ listings.length === 1 ? "" : "s" }}</span>
          </div>
        </div>
      </div>

      <template v-if="section === 'listings'">
        <div v-if="listingError" class="panel empty-state riven-empty">
        <Icon name="lock" :size="26" />
        <h3>Listings unavailable</h3>
        <p>{{ listingError }}</p>
        <button class="button ghost" type="button" @click="refresh">
          <Icon name="refresh" :size="14" /> Retry
        </button>
      </div>
      <div v-else-if="!loading && !listings.length" class="panel empty-state riven-empty">
        <Icon name="trading" :size="26" />
        <h3>No active listings</h3>
        <p>Manage contracts and auctions on warframe.market.</p>
        <button class="button ghost" type="button" @click="openAuctions">
          <Icon name="external" :size="14" /> Open warframe.market
        </button>
      </div>
      <div v-else class="riven-grid">
        <article v-for="listing in listings" :key="listing.id" class="riven-card">
          <div class="riven-head">
            <span class="riven-gem"><Icon name="trading" :size="18" /></span>
            <div class="riven-title">
              <h3>{{ listing.name }}</h3>
              <span>
                {{ listing.weapon ?? listing.weapon_slug }} ·
                {{ listing.direct ? "Contract" : "Auction" }}
              </span>
            </div>
            <span class="riven-price">{{ listingPrice(listing) }}</span>
          </div>
          <div class="riven-stats">
            <div
              v-for="stat in listing.stats"
              :key="`${listing.id}-${stat.url_name}-${stat.positive}`"
              class="riven-stat"
              :class="{ negative: !stat.positive }"
            >
              <span>{{ stat.name }}</span>
              <strong>{{ statText(stat) }}</strong>
            </div>
            <div v-if="!listing.stats.length" class="riven-stat">
              <span>No stats reported</span>
            </div>
          </div>
          <div class="riven-foot">
            <span>{{ listedAgo(listing.created) }}</span>
            <span v-if="!listing.direct">Start {{ formatPlatinum(listing.starting_price) }}</span>
            <span v-if="listing.top_bid != null" class="riven-bid">
              Bid {{ formatPlatinum(listing.top_bid) }}
            </span>
            <span v-if="listing.buyout_price != null">Buyout {{ formatPlatinum(listing.buyout_price) }}</span>
          </div>
        </article>
      </div>

      </template>

      <template v-else>
        <div v-if="ownedError" class="panel empty-state riven-empty">
        <Icon name="lock" :size="26" />
        <h3>Snapshot unavailable</h3>
        <p>{{ ownedError }}</p>
      </div>
      <div v-else-if="!loading && !filteredOwned.length" class="panel empty-state riven-empty">
        <Icon name="search" :size="26" />
        <h3>{{ query ? "No rivens match" : "No rivens in the snapshot" }}</h3>
        <p v-if="query">Nothing matches “{{ query }}”. Try a weapon, riven name, or stat.</p>
        <p v-else>Open the inventory in Warframe and refresh so the snapshot sees them.</p>
        <button v-if="query" class="button ghost" type="button" @click="query = ''">
          Clear search
        </button>
      </div>
      <div v-else class="riven-grid">
        <article v-for="riven in filteredOwned" :key="riven.id" class="riven-card">
          <div class="riven-head">
            <span class="riven-gem"><Icon name="rivens" :size="18" /></span>
            <div class="riven-title">
              <h3 v-if="riven.veiled">{{ riven.name }}</h3>
              <h3 v-else>
                <span>{{ riven.weapon ?? riven.weapon_type ?? "Riven" }}</span>
                <span class="riven-generated">{{ riven.name }}</span>
              </h3>
              <span v-if="!riven.veiled">
                {{ riven.polarity }}<template v-if="riven.mastery"> · MR {{ riven.mastery }}</template>
              </span>
              <span v-else>{{ riven.weapon_type }}</span>
            </div>
            <span class="riven-price" :class="{ veiled: riven.veiled }">
              {{ riven.veiled ? "Veiled" : formatRank(riven.rank) }}
            </span>
          </div>
          <div class="riven-stats">
            <template v-if="riven.veiled">
              <div class="riven-stat riven-challenge">
                <span>Unveil</span>
                <strong>{{ riven.challenge ?? "Locked" }}</strong>
              </div>
            </template>
            <template v-else>
              <div
                v-for="stat in riven.stats"
                :key="`${riven.id}-${stat.slug}-${stat.positive}`"
                class="riven-stat"
                :class="{ negative: !stat.positive }"
              >
                <span>{{ stat.name }}</span>
                <strong>{{ statText(stat) }}</strong>
              </div>
              <div v-if="!riven.stats.length" class="riven-stat">
                <span>No stats in the fingerprint</span>
              </div>
            </template>
          </div>
          <div class="riven-foot">
            <span><Icon name="refresh" :size="12" /> {{ riven.rerolls }} rerolls</span>
            <span v-if="!riven.veiled">Rank {{ riven.rank }}</span>
            <span v-if="riven.weapon_type">{{ riven.weapon_type }}</span>
          </div>
        </article>
      </div>
      </template>
    </div>
  </div>
</template>
