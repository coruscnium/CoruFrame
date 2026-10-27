<script setup lang="ts">
import { onMounted, onUnmounted } from "vue";
import { RouterView } from "vue-router";
import SideNav from "./components/SideNav.vue";
import RightRail from "./components/RightRail.vue";
import Icon from "./components/Icon.vue";
import { app } from "./stores/app";
import {
  catalogStore,
  loadItemDbStatus,
  maybeAutoUpdateItemDb,
} from "./stores/catalog";
import { loadInventory, startInventoryWatch } from "./stores/inventory";
import { initPresence } from "./stores/presence";
import {
  dismissTradeNotice,
  startTradeWatch,
  stopTradeWatch,
  tradeNotice,
} from "./stores/trades";
import {
  applyAppScale,
  dismissSnapshotNotice,
  initSnapshotPreference,
  snapshotNotice,
} from "./stores/ui";

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape" && app.railOpen) app.railOpen = false;
}

onMounted(async () => {
  document.addEventListener("keydown", onKeydown);
  void applyAppScale();
  // The consent check is awaited: loading first would flash "Snapshot off" and
  // read the snapshot for a user who never opted in.
  await initSnapshotPreference();
  void loadInventory();
  startInventoryWatch();
  void loadItemDbStatus();
  void initPresence();
  void startTradeWatch();
  // Let the first page paint before a background build competes with it.
  requestAnimationFrame(() => {
    window.setTimeout(() => void maybeAutoUpdateItemDb(), 0);
  });
});

onUnmounted(() => {
  document.removeEventListener("keydown", onKeydown);
  stopTradeWatch();
});
</script>

<template>
  <div class="shell">
    <SideNav />
    <main class="workspace">
      <RouterView />
    </main>
    <div
      class="rail-backdrop"
      :class="{ show: app.railOpen }"
      @click="app.railOpen = false"
    ></div>
    <RightRail />
    <div v-if="snapshotNotice" class="app-toast snapshot-notice" role="status">
      <span>Inventory snapshot is on because this install already had one.</span>
      <button type="button" class="snapshot-notice-dismiss" @click="dismissSnapshotNotice">
        Dismiss
      </button>
    </div>
    <div v-if="tradeNotice" class="app-toast trade-notice" role="status">
      <Icon name="trading" :size="13" />
      <span>{{ tradeNotice }}</span>
      <button type="button" class="snapshot-notice-dismiss" @click="dismissTradeNotice">
        Dismiss
      </button>
    </div>
    <div v-if="catalogStore.updating" class="app-toast" role="status">
      <Icon name="refresh" :size="13" /> Updating item data…
    </div>
  </div>
</template>
