<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import Icon from "../components/Icon.vue";
import PageHeader from "../components/PageHeader.vue";
import PageBar from "../components/PageBar.vue";
import SearchField from "../components/SearchField.vue";
import TierMark from "../components/TierMark.vue";
import WfIcon from "../components/WfIcon.vue";
import { relicEntries, type RelicEntryDto } from "../lib/api";
import { relicImage } from "../lib/equipment";
import { formatDucats, formatPercent, formatPlatinum } from "../lib/format";
import { useCompactTabs } from "../lib/useCompactTabs";
import { inventory, loadInventory } from "../stores/inventory";
import { initMarket, loadCachedQuotes, market } from "../stores/market";

const tiers = ["All", "Lith", "Meso", "Neo", "Axi"];
const refinements = ["All", "Intact", "Exceptional", "Flawless", "Radiant"];
const tier = ref("All");
const state = ref("All");
const query = ref("");
const sortKey = ref<"plat" | "ducats" | "name">("plat");
const sortDir = ref<"asc" | "desc">("desc");

const entries = ref<RelicEntryDto[]>([]);
const loading = ref(false);
const error = ref("");

// The tab icons follow the chosen refinement, so the era thumbnails show the
// same card the list prices.
const tierImages = computed(() =>
  Object.fromEntries(
    tiers.map((option) => [
      option,
      relicImage(option, state.value === "All" ? "Intact" : state.value),
    ]),
  ),
);
const tabsEl = ref<HTMLElement | null>(null);
const tabsCompact = useCompactTabs(tabsEl);

// Equipment names are "Axi A1 Intact"; the backend splits the same name into
// tier, relic name and state, so both sides meet on this form.
function keyOf(text: string) {
  return text.toLowerCase().replace(/[^a-z0-9]/g, "");
}
const entryByKey = computed(
  () =>
    new Map(
      entries.value.map((entry) => [
        keyOf(`${entry.tier} ${entry.name} ${entry.state}`),
        entry,
      ]),
    ),
);

const owned = computed(() =>
  (inventory.snapshot?.items ?? []).filter(
    (item) => item.kind === "relic" && item.count > 0,
  ),
);

interface PlanReward {
  name: string;
  chance: number;
  price: number | null;
  ducats: number | null;
}

interface PlanRow {
  id: string;
  tier: string;
  name: string;
  state: string;
  count: number;
  rewards: PlanReward[];
  expectedPlat: number | null;
  expectedDucats: number;
}

const plans = computed<PlanRow[]>(() => {
  const rows: PlanRow[] = [];
  for (const item of owned.value) {
    const entry = entryByKey.value.get(keyOf(item.item.name));
    if (!entry) continue;
    if (entry.tier.toLowerCase() === "requiem") continue;
    let priced = false;
    let platinum = 0;
    let ducats = 0;
    const rewards = entry.rewards.map((reward) => {
      const quote = reward.slug ? market.quotes[reward.slug] : undefined;
      const price = quote?.platinum ?? null;
      if (price != null) {
        priced = true;
        platinum += (reward.chance / 100) * price;
      }
      if (reward.ducats != null) ducats += (reward.chance / 100) * reward.ducats;
      return {
        name: reward.name,
        chance: reward.chance,
        price,
        ducats: reward.ducats,
      };
    });
    rows.push({
      id: item.item.name,
      tier: entry.tier,
      name: entry.name,
      state: entry.state,
      count: item.count,
      rewards,
      expectedPlat: priced ? Number(platinum.toFixed(1)) : null,
      expectedDucats: ducats,
    });
  }
  return rows;
});

function labelOf(relic: PlanRow) {
  return `${relic.tier} ${relic.name} ${relic.state}`;
}

const visible = computed(() => {
  const terms = query.value.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const list = plans.value.filter((relic) => {
    if (tier.value !== "All" && relic.tier !== tier.value) return false;
    if (state.value !== "All" && relic.state !== state.value) return false;
    if (!terms.length) return true;
    const text = [relic.tier, relic.name, relic.state, ...relic.rewards.map((reward) => reward.name)]
      .join(" ")
      .toLowerCase();
    return terms.every((term) => text.includes(term));
  });
  const direction = sortDir.value === "asc" ? 1 : -1;
  list.sort((left, right) => {
    if (sortKey.value === "name") {
      return labelOf(left).localeCompare(labelOf(right)) * direction;
    }
    const pick = (relic: PlanRow) =>
      sortKey.value === "plat" ? relic.expectedPlat : relic.expectedDucats;
    const leftValue = pick(left);
    const rightValue = pick(right);
    if (leftValue == null && rightValue == null) {
      return labelOf(left).localeCompare(labelOf(right));
    }
    if (leftValue == null) return 1;
    if (rightValue == null) return -1;
    if (leftValue === rightValue) {
      return labelOf(left).localeCompare(labelOf(right));
    }
    return (leftValue - rightValue) * direction;
  });
  return list;
});

const best = computed(() => {
  let top: PlanRow | null = null;
  for (const relic of visible.value) {
    if (relic.expectedPlat == null) continue;
    if (!top || relic.expectedPlat > (top.expectedPlat ?? 0)) top = relic;
  }
  return top;
});

// The counts live in the header: Collection and Relic have no card row.
const summary = computed(() => {
  if (!inventory.snapshot) return "Reading your inventory…";
  if (!inventory.snapshot.linked) return "Link your account to plan from your relics";
  if (!owned.value.length) return "No owned relics found";
  const count = `${visible.value.length} relic${visible.value.length === 1 ? "" : "s"}`;
  const value = best.value
    ? `${formatPlatinum(best.value.expectedPlat)} best`
    : "no prices yet";
  const refinement = state.value === "All" ? "all refinements" : `${state.value} refinement`;
  return `${count} · ${value} · ${refinement}`;
});

let loadSeq = 0;

async function loadPlans() {
  const names = [...new Set(owned.value.map((item) => item.item.name))];
  if (!names.length) {
    entries.value = [];
    return;
  }
  const seq = ++loadSeq;
  loading.value = true;
  error.value = "";
  try {
    await initMarket();
    const rows = await relicEntries(names);
    if (seq !== loadSeq) return;
    entries.value = rows;
    const slugs = rows.flatMap((entry) =>
      entry.rewards.flatMap((reward) => (reward.slug ? [reward.slug] : [])),
    );
    void loadCachedQuotes(slugs);
  } catch (reason) {
    if (seq === loadSeq) error.value = String(reason);
  } finally {
    if (seq === loadSeq) loading.value = false;
  }
}

watch(
  () => owned.value.map((item) => item.item.name).join("\n"),
  () => void loadPlans(),
  { immediate: true },
);

onMounted(() => {
  void loadInventory();
});
</script>

<template>
  <div class="view">
    <PageBar>
      <template #actions>
        <SearchField
          v-model="query"
          placeholder="Search relics or rewards…"
          label="Search relics"
        />
        <label class="select-field">
          <Icon name="sparkles" :size="15" />
          <select v-model="state" aria-label="Refinement">
            <option v-for="option in refinements" :key="option">{{ option }}</option>
          </select>
        </label>
        <label class="select-field">
          <Icon name="stats" :size="15" />
          <select v-model="sortKey" aria-label="Sort relics">
            <option value="plat">Plat average</option>
            <option value="ducats">Ducat average</option>
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
      </template>

      <template #tabs>
        <div
          ref="tabsEl"
          class="inventory-tabs"
          :class="{ compact: tabsCompact }"
          role="tablist"
          aria-label="Relic tiers"
        >
          <button
            v-for="option in tiers"
            :key="option"
            type="button"
            role="tab"
            class="tier-tab"
            :class="{ active: tier === option }"
            :aria-selected="tier === option"
            :aria-label="option"
            :title="option"
            @click="tier = option"
          >
            <img
              v-if="tierImages[option]"
              class="tier-tab-thumb"
              :src="tierImages[option]!"
              alt=""
              referrerpolicy="no-referrer"
              loading="lazy"
              decoding="async"
            />
            <WfIcon v-else name="relics" :size="16" />
            <span class="tab-label">{{ option }}</span>
          </button>
        </div>
      </template>
    </PageBar>

    <div class="content">
      <PageHeader
        title="Relic planner"
        :description="summary"
        icon="relics"
      />

      <div v-if="loading && !plans.length" class="panel empty-state">
        <Icon name="relics" :size="30" />
        <h3>Reading relic drop tables</h3>
      </div>

      <div v-else-if="!inventory.snapshot" class="panel empty-state">
        <Icon name="relics" :size="30" />
        <h3>No inventory yet</h3>
        <p>Open Warframe to read your relics.</p>
      </div>

      <div v-else-if="!inventory.snapshot.linked" class="panel empty-state">
        <Icon name="relics" :size="30" />
        <h3>Inventory not linked</h3>
        <p>Link your account to plan from your relics.</p>
      </div>

      <div v-else-if="error" class="panel empty-state">
        <Icon name="refresh" :size="30" />
        <h3>Relic data unavailable</h3>
        <p>{{ error }}</p>
      </div>

      <div v-else-if="!visible.length" class="panel empty-state">
        <Icon name="search" :size="30" />
        <h3>
          {{
            !owned.length
              ? "No relics in your inventory"
              : plans.length
                ? "No relics match"
                : "No relic drop data"
          }}
        </h3>
        <p v-if="!owned.length">Relics appear here once the inventory scan finds them.</p>
        <p v-else-if="plans.length">Try a different relic, tier, refinement, or reward.</p>
        <p v-else>Warframe's drop tables did not include the relics in your inventory.</p>
        <button
          v-if="query || tier !== 'All' || state !== 'All'"
          class="button ghost"
          type="button"
          @click="query = ''; tier = 'All'; state = 'All'"
        >
          Clear filters
        </button>
      </div>

      <div v-else class="relic-grid">
        <article v-for="relic in visible" :key="relic.id" class="relic-card">
          <div class="relic-head">
            <TierMark
              :tier="relic.tier"
              :refinement="relic.state"
              class="tier-mark-lg"
            />
            <div class="relic-title">
              <h3>{{ relic.tier }} {{ relic.name }}</h3>
              <span>
                {{ relic.state }} · {{ relic.count }} owned · {{ relic.rewards.length }} rewards
              </span>
            </div>
            <div class="relic-value">
              <strong>{{ formatPlatinum(relic.expectedPlat) }}</strong>
              <span>{{ formatDucats(Math.round(relic.expectedDucats)) }}</span>
            </div>
          </div>
          <div class="relic-rewards">
            <div v-for="reward in relic.rewards" :key="reward.name" class="relic-reward">
              <span class="relic-reward-name">{{ reward.name }}</span>
              <span class="relic-chance">{{ formatPercent(reward.chance) }}</span>
              <span class="relic-price">{{ formatPlatinum(reward.price) }}</span>
            </div>
          </div>
        </article>
      </div>
    </div>
  </div>
</template>
