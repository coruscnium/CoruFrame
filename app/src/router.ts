import { createRouter, createWebHashHistory } from "vue-router";
import CollectionView from "./views/CollectionView.vue";
import InventoryView from "./views/InventoryView.vue";
import RelicPlannerView from "./views/RelicPlannerView.vue";
import RivensView from "./views/RivensView.vue";
import TradingView from "./views/TradingView.vue";
import HelpView from "./views/HelpView.vue";
import SettingsView from "./views/SettingsView.vue";

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: "/", redirect: "/mastery" },
    { path: "/mastery", name: "collection", component: CollectionView },
    { path: "/inventory", name: "inventory", component: InventoryView },
    { path: "/relics", name: "relics", component: RelicPlannerView },
    { path: "/rivens", name: "rivens", component: RivensView },
    { path: "/trading", name: "trading", component: TradingView },
    { path: "/about", name: "about", component: HelpView },
    { path: "/settings", name: "settings", component: SettingsView },
    { path: "/:pathMatch(.*)*", redirect: "/mastery" },
  ],
});
