<script setup lang="ts">
import { computed, onMounted } from "vue";
import TopBar from "../components/TopBar.vue";
import ItemGrid from "../components/ItemGrid.vue";
import Icon from "../components/Icon.vue";
import ItemArt from "../components/ItemArt.vue";
import PageHeader from "../components/PageHeader.vue";
import { app, selectItem } from "../stores/app";
import { catalogStore, loadCatalog, updateItemDb } from "../stores/catalog";
import { inventory, loadInventory, snapshotSchemaIsNewer } from "../stores/inventory";
import { completionItems, type CompletionItem } from "../stores/completion";
import { iconUrl } from "../stores/market";
import { equipmentKind } from "../lib/equipment";
import { isTauri } from "../lib/api";
import { ageSince, formatAge, formatClock } from "../lib/format";
import { categoryIcon, type WfIconName } from "../lib/wficons";
import type { Category } from "../data/mock";

const titles: Partial<Record<Category, string>> = {
  Warframes: "Warframe",
  Primary: "Primary",
  Secondary: "Secondary",
  Melee: "Melee",
  Companions: "Companion",
  "Companion Weapons": "Companion Weapon",
  Archwing: "Archwing",
  Archguns: "Archguns",
  Archmelee: "Archmelee",
  Necramech: "Necramech",
  Amps: "Amp",
};

const kinds: Partial<Record<Category, string>> = {
  Warframes: "warframe",
  Primary: "primary",
  Secondary: "secondary",
  Melee: "melee",
  Companions: "companion",
  "Companion Weapons": "companion_weapon",
  Archwing: "archwing",
  Archguns: "archgun",
  Archmelee: "archmelee",
  Necramech: "necramech",
  Amps: "amp",
};

const title = computed(() => titles[app.category] ?? "Collection");
const pageIcon = computed<WfIconName>(() => categoryIcon(app.category));

// Which snapshot array each collection section reads. A section whose array is
// absent renders as "not captured", never as zero owned. Non-inventory tabs are
// excluded so a new collection category fails the build until it is mapped.
type CollectionCategory = Exclude<Category, "Mods" | "Arcanes" | "Relics" | "Ayatan">;

const categoryArray: Record<CollectionCategory, string> = {
  Warframes: "Suits",
  Primary: "LongGuns",
  Secondary: "Pistols",
  Melee: "Melee",
  Companions: "Sentinels",
  "Companion Weapons": "SentinelWeapons",
  Archwing: "SpaceSuits",
  Archguns: "SpaceGuns",
  Archmelee: "SpaceMelee",
  Necramech: "MechSuits",
  Amps: "OperatorAmps",
};

function snapshotArray(category: Category): string | undefined {
  return (categoryArray as Partial<Record<Category, string>>)[category];
}

const arrayLabels: Record<string, string> = {
  MiscItems: "misc items",
  Suits: "warframes",
  LongGuns: "primaries",
  Pistols: "secondaries",
  Melee: "melee",
  XPInfo: "mastery XP",
  Sentinels: "companions",
  SentinelWeapons: "companion weapons",
  KubrowPets: "pets",
  MechSuits: "necramechs",
  SpaceSuits: "archwings",
  SpaceGuns: "archguns",
  SpaceMelee: "archmelee",
  OperatorAmps: "amps",
  RawUpgrades: "mods",
  Upgrades: "mod ranks",
};

const missing = computed(() => inventory.snapshot?.missing ?? []);
const notCaptured = computed(() => {
  const array = snapshotArray(app.category);
  return !!array && missing.value.includes(array);
});
const incomplete = computed(() => inventory.snapshot?.complete === false);
const missingLabel = computed(() =>
  missing.value.map((key) => arrayLabels[key] ?? key).join(", "),
);

const sectionLabels: Record<string, string> = {
  MiscItems: "Misc items",
  Suits: "Warframes",
  LongGuns: "Primaries",
  Pistols: "Secondaries",
  Melee: "Melee",
  XPInfo: "Mastery XP",
  Sentinels: "Companions",
  SentinelWeapons: "Companion weapons",
  KubrowPets: "Pets",
  MechSuits: "Necramech",
  SpaceSuits: "Archwing",
  SpaceGuns: "Archguns",
  SpaceMelee: "Archmelee",
  OperatorAmps: "Amps",
};

function observedLabel(at: number | null): string {
  const clock = formatClock(at);
  return clock ? ` from ${clock}` : "";
}

const sectionOrder = Object.keys(sectionLabels);

const carriedSections = computed(() =>
  Object.entries(inventory.snapshot?.array_status ?? {})
    .filter(([key, status]) => status.source === "carried" && key in sectionLabels)
    .sort(([left], [right]) => sectionOrder.indexOf(left) - sectionOrder.indexOf(right))
    .map(([key, status]) => `${sectionLabels[key]}${observedLabel(status.observed_at)}`),
);

const synthesizedSections = computed(() =>
  Object.entries(inventory.snapshot?.array_status ?? {})
    .filter(([key, status]) => status.source === "synthesized" && key in sectionLabels)
    .sort(([left], [right]) => sectionOrder.indexOf(left) - sectionOrder.indexOf(right))
    .map(([key]) => sectionLabels[key]),
);

// One compact line for the section in front of the user: the global banner is
// easy to miss when the page is scrolled.
const sectionProvenance = computed(() => {
  const array = snapshotArray(app.category);
  if (!array) return "";
  const status = inventory.snapshot?.array_status?.[array];
  if (!status) return "";
  const label = sectionLabels[array] ?? array;
  if (status.source === "carried") {
    const clock = formatClock(status.observed_at);
    return `${label} · carried${clock ? ` from ${clock}` : " from an earlier capture"}`;
  }
  if (status.source === "synthesized") {
    return `${label} · not present in the latest sync`;
  }
  return "";
});

const lastScan = computed(() => inventory.status?.last_scan ?? null);
const lastScanFailed = computed(() => lastScan.value != null && !lastScan.value.complete);
const lastScanReason = computed(
  () => lastScan.value?.error ?? lastScan.value?.limit ?? "incomplete",
);
const snapshotUnreadable = computed(() => inventory.status?.previous_unreadable === true);
const schemaNewer = computed(() => snapshotSchemaIsNewer());
const capturedAge = computed(() =>
  formatAge(
    ageSince(inventory.status?.captured_at ?? inventory.snapshot?.captured_at ?? null),
  ),
);

function kitgunSlot(uniqueName: string, itemType: string | null): string | null {
  if (itemType !== "Kitgun") return null;
  if (uniqueName.endsWith("#primary")) return "P";
  if (uniqueName.endsWith("#secondary")) return "S";
  return null;
}

const masteryAvailable = computed(() => inventory.snapshot?.mastery_available === true);
const xp = computed(() => inventory.snapshot?.mastery.xp ?? {});

const base = computed(() => {
  const kind = kinds[app.category];
  if (!kind) return [];
  const query = app.search.trim().toLowerCase();
  return completionItems.value.filter((item) => {
    if (equipmentKind(item.card.item) !== kind) return false;
    if (query && !item.card.item.name.toLowerCase().includes(query)) return false;
    return true;
  });
});

const filtered = computed(() =>
  base.value.filter((item) => {
    if (app.filter === "owned" && !item.owned) return false;
    if (app.filter === "missing" && item.owned) return false;
    if (app.filter === "mastered" && !item.mastered) return false;
    if (app.filter === "unmastered" && item.mastered) return false;
    if (app.filter === "vaulted" && !item.card.item.vaulted) return false;
    return true;
  }),
);

const counts = computed(() => {
  const all = base.value;
  const owned = all.filter((item) => item.owned).length;
  const mastered = all.filter((item) => item.mastered).length;
  const vaulted = all.filter((item) => item.card.item.vaulted).length;
  return { total: all.length, owned, mastered, vaulted, missing: all.length - owned };
});

function percentOf(item: CompletionItem): number | null {
  if (!masteryAvailable.value) return null;
  const cap = item.card.item.mastery_cap;
  if (!cap) return null;
  const value = xp.value[item.card.item.unique_name] ?? 0;
  return Math.min(100, Math.max(0, Math.round((value / cap) * 100)));
}

function xpTitle(item: CompletionItem) {
  const cap = item.card.item.mastery_cap;
  if (!cap) return "";
  const value = xp.value[item.card.item.unique_name] ?? 0;
  return `${value.toLocaleString()} / ${cap.toLocaleString()} XP`;
}

const listItems = computed(() => {
  const list = [...filtered.value];
  const direction = app.sortAsc ? 1 : -1;
  if (app.filter === "unmastered") {
    list.sort((left, right) => {
      const progress = ((percentOf(left) ?? 0) - (percentOf(right) ?? 0)) * direction;
      if (progress) return progress;
      if (left.owned !== right.owned) return Number(right.owned) - Number(left.owned);
      return left.card.item.name.localeCompare(right.card.item.name);
    });
  } else {
    list.sort(
      (left, right) =>
        left.card.item.name.localeCompare(right.card.item.name) * direction,
    );
  }
  return list;
});

const cards = computed(() => listItems.value.map((item) => item.card));

const ownedNames = computed(
  () =>
    new Set(
      (inventory.snapshot?.items ?? [])
        .filter((item) => item.resolved)
        .map((item) => item.item.unique_name),
    ),
);
const masteredNames = computed(
  () => new Set(inventory.snapshot?.mastery.mastered ?? []),
);


onMounted(async () => {
  await loadCatalog();
  void loadInventory();
});
</script>

<template>
  <div class="view catalog-view collection-view">
    <TopBar />
    <div class="content">
      <PageHeader :title="title" :icon="pageIcon">
        <template #description>
          <template v-if="!notCaptured">
            <button type="button" class="stat-button" @click="app.filter = 'owned'">
              {{ counts.owned }} of {{ counts.total }} owned
            </button>
            <span class="stat-sep" aria-hidden="true">·</span>
            <button type="button" class="stat-button" @click="app.filter = 'missing'">
              {{ counts.missing }} missing
            </button>
            <span class="stat-sep" aria-hidden="true">·</span>
            <button type="button" class="stat-button" @click="app.filter = 'vaulted'">
              {{ counts.vaulted }} vaulted
            </button>
            <template v-if="masteryAvailable">
              <span class="stat-sep" aria-hidden="true">·</span>
              <button type="button" class="stat-button" @click="app.filter = 'mastered'">
                {{ counts.mastered }} mastered
              </button>
            </template>
          </template>
          <span v-else class="muted">Ownership not captured in this snapshot</span>
        </template>
      </PageHeader>

      <p v-if="sectionProvenance" class="muted">{{ sectionProvenance }}</p>

      <div v-if="snapshotUnreadable" class="warning-banner collection-warning">
        <Icon name="shield" :size="16" />
        <div>
          <strong>Snapshot unreadable.</strong>
          <p>The cached snapshot could not be read. A complete scan will replace it.</p>
        </div>
      </div>

      <div v-else-if="lastScanFailed" class="warning-banner collection-warning">
        <Icon name="clock" :size="16" />
        <div>
          <strong>Last scan could not read the profile.</strong>
          <p>
            Reason <code>{{ lastScanReason }}</code>. Showing the capture from {{ capturedAge }}.
          </p>
        </div>
      </div>

      <div v-if="schemaNewer" class="warning-banner collection-warning">
        <Icon name="shield" :size="16" />
        <div>
          <strong>Snapshot format is newer.</strong>
          <p>Written by a newer CoruFrame; some sections may be ignored.</p>
        </div>
      </div>

      <div v-if="incomplete" class="warning-banner collection-warning">
        <Icon name="shield" :size="16" />
        <div>
          <strong>Snapshot incomplete.</strong>
          <p>
            Missing {{ missingLabel || "some arrays" }}. Launch through the wrapper and wait for a
            complete scan.
          </p>
        </div>
      </div>

      <div v-else-if="!masteryAvailable" class="warning-banner collection-warning">
        <Icon name="shield" :size="16" />
        <div>
          <strong>Mastery XP is not captured.</strong>
          <p>Launch through the wrapper and capture a snapshot on login.</p>
        </div>
      </div>

      <div v-if="carriedSections.length || synthesizedSections.length" class="warning-banner collection-warning">
        <Icon name="clock" :size="16" />
        <div>
          <strong>Some sections are older than this snapshot.</strong>
          <p v-if="carriedSections.length">Earlier capture: {{ carriedSections.join(", ") }}.</p>
          <p v-if="synthesizedSections.length">
            Not in the latest sync: {{ synthesizedSections.join(", ") }}. Shown as none owned.
          </p>
        </div>
      </div>

      <div v-if="catalogStore.loading && !catalogStore.cards.length" class="panel empty-state">
        <Icon name="clock" :size="30" />
        <h3>Loading the item database…</h3>
      </div>
      <div v-else-if="catalogStore.error" class="panel empty-state">
        <Icon name="database" :size="30" />
        <h3>Item database unavailable</h3>
        <p>{{ catalogStore.updateError || catalogStore.error }}</p>
        <button
          class="button ghost"
          type="button"
          :disabled="!isTauri() || catalogStore.updating"
          @click="updateItemDb(true)"
        >
          <Icon name="refresh" :size="14" />
          {{ catalogStore.updating ? "Updating…" : "Update item database" }}
        </button>
      </div>

      <div v-else-if="notCaptured" class="panel empty-state">
        <Icon name="database" :size="30" />
        <h3>Not captured in this snapshot</h3>
        <p>Missing <code>{{ snapshotArray(app.category) }}</code>. A complete scan will fill it in.</p>
      </div>

      <template v-else-if="app.layout === 'grid'">
        <ItemGrid
          :cards="cards"
          :owned-names="ownedNames"
          :mastered-names="masteredNames"
          :component-counts="inventory.snapshot?.component_counts"
        />
      </template>

      <div v-else class="panel collection-panel">
        <div class="completion-list">
          <div
            v-for="item in listItems"
            :key="item.card.item.unique_name"
            class="completion-row"
            :class="{
              selected: app.selected?.item.unique_name === item.card.item.unique_name,
            }"
          >
            <div
              class="completion-summary"
              role="button"
              tabindex="0"
              :aria-label="`Select ${item.card.item.name}`"
              @click="selectItem(item.card)"
              @keydown.enter.prevent="selectItem(item.card)"
              @keydown.space.prevent="selectItem(item.card)"
            >
              <span
                class="mastery-emblem"
                :class="{ 'has-image': item.card.item.image_url }"
                :style="{ '--item-hue': String(item.card.item.name.length * 29) }"
              >
                <ItemArt
                  :image="item.card.item.image_url"
                  :market-icon="
                    item.card.market
                      ? iconUrl(item.card.market.thumb ?? item.card.market.icon)
                      : null
                  "
                  :kind="item.card.item.kind"
                  :icon-size="20"
                  alt=""
                />
              </span>
              <span class="completion-name">
                <strong>{{ item.card.item.name }}</strong>
                <span v-if="item.card.item.vaulted || item.card.item.mastery_req">
                  <template v-if="item.card.item.vaulted">Vaulted</template>
                  <template v-if="item.card.item.vaulted && item.card.item.mastery_req"> · </template>
                  <template v-if="item.card.item.mastery_req">MR {{ item.card.item.mastery_req }}</template>
                </span>
                <span
                  v-if="kitgunSlot(item.card.item.unique_name, item.card.item.item_type)"
                  class="badge neutral kitgun-slot"
                  :title="
                    kitgunSlot(item.card.item.unique_name, item.card.item.item_type) === 'P'
                      ? 'Kitgun Primary'
                      : 'Kitgun Secondary'
                  "
                >
                  {{ kitgunSlot(item.card.item.unique_name, item.card.item.item_type) }}
                </span>
              </span>
              <span class="owned-slot">
                <span v-if="item.owned" class="badge owned">
                  <Icon name="check" :size="11" /> Owned
                </span>
              </span>
              <span
                v-if="percentOf(item) != null"
                class="mastery-progress"
                :title="xpTitle(item)"
              >
                <span class="mastery-meter"
                  ><span :style="{ width: `${percentOf(item)}%` }"></span
                ></span>
                <span class="mastery-percent">{{ percentOf(item) }}%</span>
              </span>
              <span class="mastered-slot">
                <span v-if="item.mastered" class="badge neutral">
                  <Icon name="star" :size="11" /> Mastered
                </span>
              </span>
            </div>
          </div>
          <div v-if="!listItems.length" class="empty-state">
            <Icon name="search" :size="30" />
            <h3>No items match</h3>
            <p>Try another category, filter, or search.</p>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
