<script setup lang="ts">
import { computed, onMounted } from "vue";
import { RouterLink } from "vue-router";
import Icon from "./Icon.vue";
import WfIcon from "./WfIcon.vue";
import type { WfIconName } from "../lib/wficons";
import { app } from "../stores/app";
import { presence, setPresence, type PresenceStatus } from "../stores/presence";
import { account, loadAccount, refreshAccount } from "../stores/account";
import { catalogStore, refreshData } from "../stores/catalog";
import { inventory, rescanInventory } from "../stores/inventory";
import { ensureWorldstate, loadWorldstate, worldstateState } from "../stores/worldstate";
import { snapshotChecking, snapshotEnabled } from "../stores/ui";
import { ageSince, formatAge } from "../lib/format";

const version = __APP_VERSION__;

const items: { label: string; short: string; icon: WfIconName; to: string }[] = [
  { label: "Collection", short: "COL", icon: "collection", to: "/mastery" },
  { label: "Inventory", short: "INV", icon: "inventory", to: "/inventory" },
  { label: "Relic planner", short: "REL", icon: "relics", to: "/relics" },
  { label: "Rivens", short: "RIV", icon: "rivens", to: "/rivens" },
  { label: "Trading", short: "TRD", icon: "trading", to: "/trading" },
];

const statusLabels: Record<PresenceStatus, string> = {
  online: "Online",
  ingame: "Online in-game",
  auto: "Auto",
  invisible: "Invisible",
};

const dotStatus = computed(() =>
  presence.status === "auto" ? presence.effective || "invisible" : presence.status,
);

const autoLabel = computed(() => {
  if (presence.status !== "auto" || !presence.effective) return "Auto";
  return `Auto (${presence.effective === "ingame" ? "in-game" : presence.effective})`;
});

function onStatus(event: Event) {
  setPresence((event.target as HTMLSelectElement).value as PresenceStatus);
}

// Item data is the downloaded item database; the in-game data is the read-only
// inventory scan. They refresh independently, so each gets its own button.
const dataState = computed(() => {
  if (catalogStore.updating) return { label: "Syncing", tone: "syncing" };
  const status = catalogStore.status;
  if (!status) return { label: "Checking", tone: "syncing" };
  if (!status.available) return { label: "Missing", tone: "unlinked" };
  return { label: "Ready", tone: "linked" };
});

const updatedLabel = computed(() => {
  const status = catalogStore.status;
  if (!status) return "Checking item data…";
  if (!status.available) return "No item database";
  return `Updated ${formatAge(status.age_secs)}`;
});

const snapshotState = computed(() => {
  if (snapshotChecking.value) return { label: "Checking", tone: "syncing" };
  if (inventory.rescanning) return { label: "Scanning", tone: "syncing" };
  if (inventory.loading) return { label: "Syncing", tone: "syncing" };
  if (!snapshotEnabled.value) return { label: "Off", tone: "checking" };
  if (!app.snapshotChecked) return { label: "Checking", tone: "syncing" };
  if (!app.snapshotLinked) return { label: "Not linked", tone: "unlinked" };
  if (inventory.error) return { label: "Error", tone: "unlinked" };
  const lastScan = inventory.status?.last_scan;
  if (lastScan?.error) return { label: "Error", tone: "unlinked" };
  if (lastScan?.limit_blocking) return { label: "Limited", tone: "syncing" };
  return { label: "Ready", tone: "linked" };
});

const snapshotAbsolute = computed(() => {
  const parts: string[] = [];
  const captured = inventory.snapshot?.captured_at;
  const checked = inventory.status?.last_scan_at;
  if (captured) parts.push(`Changed ${new Date(captured * 1000).toLocaleString()}`);
  if (checked) parts.push(`Scanned ${new Date(checked * 1000).toLocaleString()}`);
  return parts.join(" · ");
});

const snapshotMeta = computed(() => {
  if (snapshotChecking.value) return "Checking the preference…";
  if (!snapshotEnabled.value) return "Enable in Settings";
  if (inventory.rescanning) return "Scanning the game…";
  if (inventory.error) return inventory.error;
  if (!app.snapshotChecked || inventory.loading) return "Reading in-game data…";
  const captured = inventory.snapshot?.captured_at;
  if (!app.snapshotLinked || !captured) return "Launch through the wrapper";
  const checked = inventory.status?.last_scan_at;
  const capturedLabel = `Changed ${formatAge(ageSince(captured))}`;
  return checked
    ? `${capturedLabel} · scanned ${formatAge(ageSince(checked))}`
    : capturedLabel;
});

const walletUnloaded = computed(
  () => account.status.storage === "unknown" && !account.status.connected,
);

const marketState = computed(() => {
  if (account.loading || walletUnloaded.value) {
    return { label: "Checking", tone: "syncing" };
  }
  if (!account.status.connected) return { label: "Public", tone: "checking" };
  if (!account.status.verified) return { label: "Unverified", tone: "syncing" };
  return { label: "Linked", tone: "linked" };
});

const marketMeta = computed(() => {
  if (account.loading || walletUnloaded.value) return "Checking the wallet…";
  if (account.status.connected) return "Orders and prices";
  if (account.status.needs_unlock) return "Wallet locked · Retry with refresh";
  return "Prices only · sign in";
});

const worldState = computed(() => {
  if (worldstateState.loading && !worldstateState.data) {
    return { label: "Checking", tone: "syncing" };
  }
  if (!worldstateState.data) return { label: "Offline", tone: "unlinked" };
  return worldstateState.data.stale
    ? { label: "Projected", tone: "syncing" }
    : { label: "Live", tone: "linked" };
});

const worldMeta = computed(() => {
  const data = worldstateState.data;
  if (!data) return "No data";
  const at = Date.parse(data.upstream);
  if (!Number.isFinite(at)) return "Updated just now";
  const age = Math.max(0, Math.floor(Date.now() / 1000 - at / 1000));
  return `Updated ${formatAge(age)}`;
});

onMounted(() => {
  void loadAccount();
  ensureWorldstate();
});
</script>

<template>
  <aside class="sidebar">
    <RouterLink to="/" class="brand" aria-label="CoruFrame collection">
      <span class="brand-mark" aria-hidden="true">
        <svg class="brand-glyph" viewBox="0 0 40 40" fill="none">
          <path class="brand-shell" d="M20 2.5 35.5 11v18L20 37.5 4.5 29V11z" />
          <path class="brand-antennae" d="M12.5 7.5 9.5 4.5M27.5 7.5l3-3" />
          <path class="brand-face" d="M20 9l8.5 5.2v10.1L20 30l-8.5-5.7V14.2z" />
          <path class="brand-brow" d="M13.5 14.5 20 11l6.5 3.5" />
          <circle class="brand-eye" cx="20" cy="18.5" r="2.3" />
        </svg>
      </span>
      <span class="brand-copy">
        <span class="brand-name">CORUFRAME</span>
        <span class="brand-sub">Tenno companion</span>
      </span>
    </RouterLink>

    <div class="nav-section-label">Workspace</div>
    <nav class="nav" aria-label="Main sections">
      <RouterLink
        v-for="item in items"
        :key="item.to"
        :to="item.to"
        class="nav-item"
        active-class="active"
        :title="item.label"
      >
        <WfIcon :name="item.icon" :size="19" />
        <span class="nav-label">{{ item.label }}</span>
        <span class="nav-short">{{ item.short }}</span>
      </RouterLink>
    </nav>

    <div class="sidebar-spacer"></div>

    <div class="presence-card">
      <div class="presence-head">
        <span class="status-dot" :class="dotStatus"></span>
        <span>WFM status</span>
        <span class="presence-state">{{ presence.connected ? "live" : "offline" }}</span>
      </div>
      <select
        class="presence-select"
        :value="presence.status"
        aria-label="warframe.market status"
        @change="onStatus"
      >
        <option value="online">{{ statusLabels.online }}</option>
        <option value="ingame">{{ statusLabels.ingame }}</option>
        <option value="auto">{{ autoLabel }}</option>
        <option value="invisible">{{ statusLabels.invisible }}</option>
      </select>
      <p v-if="presence.error" class="presence-error">{{ presence.error }}</p>
    </div>

    <div class="status-card" aria-live="polite">
      <div class="status-head">
        <span class="status-dot" :class="marketState.tone"></span>
        <span title="warframe.market API and orders">WFM API</span>
        <span class="status-state">{{ marketState.label }}</span>
        <button
          class="status-refresh"
          type="button"
          :disabled="account.loading"
          :aria-label="
            account.status.needs_unlock
              ? 'Retry the wallet'
              : account.loading
                ? 'Refreshing warframe.market'
                : 'Refresh warframe.market'
          "
          :title="
            account.status.needs_unlock
              ? 'Retry the wallet'
              : 'Re-check the account and orders'
          "
          @click="refreshAccount()"
        >
          <Icon name="refresh" :size="13" :class="{ spinning: account.loading }" />
        </button>
      </div>
      <div class="status-meta">{{ marketMeta }}</div>

      <div class="status-head">
        <span class="status-dot" :class="worldState.tone"></span>
        <span>Worldstate</span>
        <span class="status-state">{{ worldState.label }}</span>
        <button
          class="status-refresh"
          type="button"
          :disabled="worldstateState.loading"
          :aria-label="
            worldstateState.loading ? 'Refreshing worldstate' : 'Refresh worldstate'
          "
          title="Re-fetch cycles, fissures and alerts"
          @click="loadWorldstate(true)"
        >
          <Icon
            name="refresh"
            :size="13"
            :class="{ spinning: worldstateState.loading }"
          />
        </button>
      </div>
      <div class="status-meta">{{ worldMeta }}</div>

      <div class="status-head">
        <span class="status-dot" :class="dataState.tone"></span>
        <span>Item data</span>
        <span class="status-state">{{ dataState.label }}</span>
        <button
          class="status-refresh"
          type="button"
          :disabled="catalogStore.updating"
          :aria-label="catalogStore.updating ? 'Updating item data' : 'Refresh item data'"
          :title="catalogStore.updateError || 'Refresh item data'"
          @click="refreshData()"
        >
          <Icon name="refresh" :size="13" :class="{ spinning: catalogStore.updating }" />
        </button>
      </div>
      <div class="status-meta">{{ updatedLabel }}</div>

      <div class="status-head">
        <span class="status-dot" :class="snapshotState.tone"></span>
        <span>In-game data</span>
        <span class="status-state">{{ snapshotState.label }}</span>
        <button
          class="status-refresh"
          type="button"
          :disabled="inventory.loading || inventory.rescanning || !snapshotEnabled"
          :aria-label="
            inventory.rescanning
              ? 'Rescanning in-game data'
              : 'Force a full in-game data scan'
          "
          title="Force a full inventory scan"
          @click="rescanInventory()"
        >
          <Icon
            name="refresh"
            :size="13"
            :class="{ spinning: inventory.loading || inventory.rescanning }"
          />
        </button>
      </div>
      <div class="status-meta" :title="snapshotAbsolute">{{ snapshotMeta }}</div>
    </div>

    <nav class="nav nav-bottom" aria-label="Application">
      <RouterLink to="/about" class="nav-item" active-class="active" title="Help/About">
        <Icon name="help" :size="19" class="nav-svg" />
        <span class="nav-label">Help/About</span>
        <span class="nav-short">ABT</span>
      </RouterLink>
      <RouterLink to="/settings" class="nav-item" active-class="active" title="Settings">
        <Icon name="settings" :size="19" class="nav-svg" />
        <span class="nav-label">Settings</span>
        <span class="nav-short">SET</span>
      </RouterLink>
    </nav>
    <div class="sidebar-version">v{{ version }}</div>
  </aside>
</template>
