import { shallowReactive } from "vue";
import {
  catalogCards,
  isTauri,
  itemdbStatus,
  itemdbUpdate,
  type CatalogCardDto,
  type ItemDbStatusDto,
} from "../lib/api";
import { catalog as mockCatalog, type CatalogItem } from "../data/mock";
import { loadInventory } from "./inventory";

export type CatalogCard = CatalogCardDto & {
  owned?: boolean;
  mastered?: boolean;
};

export const catalogStore = shallowReactive({
  cards: [] as CatalogCard[],
  status: null as ItemDbStatusDto | null,
  loading: false,
  updating: false,
  updateError: "",
  error: "",
  initialized: false,
  loadedAt: 0,
});

const MOCK_KIND: Record<CatalogItem["category"], string> = {
  Warframes: "warframe",
  Primary: "primary",
  Secondary: "secondary",
  Melee: "melee",
  Companions: "companion",
  "Companion Weapons": "companion_weapon",
  Archwing: "archwing",
  Archguns: "archwing",
  Archmelee: "archwing",
  Necramech: "necramech",
  Amps: "amp",
  Mods: "mod",
  Arcanes: "arcane",
  Relics: "relic",
  Ayatan: "misc",
};

const MOCK_EXTRA: {
  name: string;
  category: CatalogItem["category"];
  kind: string;
  tags: string[];
  max_rank?: number;
  amber?: number;
  cyan?: number;
  subtypes?: string[];
  bulk?: boolean;
}[] = [
  { name: "Vitality", category: "Mods", kind: "mod", tags: ["mod"], max_rank: 10, subtypes: ["regular"] },
  { name: "Continuity", category: "Mods", kind: "mod", tags: ["mod"], max_rank: 5 },
  { name: "Arcane Energize", category: "Arcanes", kind: "arcane", tags: ["arcane_enhancement"], max_rank: 5, bulk: true },
  {
    name: "Axi A5 Relic",
    category: "Relics",
    kind: "relic",
    tags: ["relic"],
    subtypes: ["intact", "exceptional", "flawless", "radiant"],
    bulk: true,
  },
  {
    name: "Ayatan Anasa Sculpture",
    category: "Ayatan",
    kind: "misc",
    tags: ["ayatan_sculpture"],
    amber: 2,
    cyan: 2,
    bulk: true,
  },
];

function slugify(name: string) {
  return name.toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_|_$/g, "");
}

function mockExtraCards(): CatalogCard[] {
  return MOCK_EXTRA.map((entry) => {
    const slug = slugify(entry.name);
    return {
      item: {
        unique_name: `mock:${slug}`,
        name: entry.name,
        category: entry.category,
        kind: entry.kind,
        item_type: null,
        image_url: null,
        tradable: true,
        masterable: false,
        mastery_req: null,
        vaulted: false,
        prime: false,
        build_price: null,
        build_time: null,
        build_quantity: null,
        skip_build_time_price: null,
        bp_cost: null,
        mastery_cap: null,
      },
      market: {
        id: slug,
        slug,
        name: entry.name,
        icon: null,
        thumb: null,
        ducats: null,
        vaulted: false,
        tradable: true,
        max_rank: entry.max_rank ?? null,
        max_charges: null,
        max_amber_stars: entry.amber ?? null,
        max_cyan_stars: entry.cyan ?? null,
        subtypes: entry.subtypes ?? [],
        tags: entry.tags,
        bulk_tradable: entry.bulk ?? false,
        trading_tax: null,
      },
      components: [],
      owned: false,
      mastered: false,
    };
  });
}

function mockCards(): CatalogCard[] {
  const equipment = mockCatalog.map((item) => {
    const slug = slugify(item.name);
    return {
      item: {
        unique_name: `mock:${item.id}`,
        name: item.name,
        category: item.category,
        kind: MOCK_KIND[item.category],
        item_type: null,
        image_url: null,
        tradable: item.price > 0,
        masterable: true,
        mastery_req: null,
        vaulted: item.vaulted,
        prime: item.name.includes("Prime"),
        build_price: null,
        build_time: null,
        build_quantity: null,
        skip_build_time_price: null,
        bp_cost: null,
        mastery_cap: null,
      },
      market:
        item.price > 0
          ? {
              id: item.id,
              slug,
              name: item.name,
              icon: null,
              thumb: null,
              ducats: item.ducats || null,
              vaulted: item.vaulted,
              tradable: true,
              max_rank: null,
              max_charges: null,
              max_amber_stars: null,
              max_cyan_stars: null,
              subtypes: [],
              tags: [],
              bulk_tradable: false,
              trading_tax: null,
            }
          : null,
      components: item.parts.map((part) => ({
        unique_name: `${slug}:${part.label}`,
        name: part.label,
        count: part.count,
        tradable: true,
        market_slug: null,
        icon: part.icon,
      })),
      owned: item.owned,
      mastered: item.mastered,
    };
  });
  return [...equipment, ...mockExtraCards()];
}

export async function loadCatalog(force = false) {
  if (catalogStore.loading || (catalogStore.initialized && !force)) return;
  catalogStore.loading = true;
  catalogStore.error = "";
  if (!isTauri()) {
    catalogStore.cards = mockCards();
    catalogStore.loadedAt = Date.now();
    catalogStore.initialized = true;
    catalogStore.loading = false;
    return;
  }
  try {
    const [cards, status] = await Promise.all([catalogCards(), itemdbStatus()]);
    catalogStore.cards = cards;
    catalogStore.status = status;
    catalogStore.loadedAt = Date.now();
  } catch (error) {
    // No mock fallback in Tauri: a missing item DB must stay visible.
    catalogStore.cards = [];
    catalogStore.error = String(error);
  } finally {
    catalogStore.loading = false;
    catalogStore.initialized = true;
  }
}

export async function loadItemDbStatus() {
  const status = await itemdbStatus();
  if (status) catalogStore.status = status;
}

// Single-flight: a second click while a build runs is ignored. A failed build
// keeps the database that already works.
export async function updateItemDb(force = false): Promise<boolean> {
  if (catalogStore.updating) return false;
  catalogStore.updating = true;
  catalogStore.updateError = "";
  try {
    const status = await itemdbUpdate(force);
    if (status) catalogStore.status = status;
    if (status?.error) {
      catalogStore.updateError = status.error;
      return false;
    }
    await Promise.all([loadCatalog(true), loadInventory(true)]);
    return true;
  } catch (error) {
    catalogStore.updateError = String(error);
    return false;
  } finally {
    catalogStore.updating = false;
  }
}

export async function refreshData(): Promise<boolean> {
  const ok = await updateItemDb(true);
  window.dispatchEvent(new Event("coruframe:refresh"));
  return ok;
}

const AUTO_UPDATE_KEY = "coruframe.itemdb.auto";
// Once per launch, after the first paint: rebuild a missing or stale database
// in the background. The current data stays on screen either way.
export async function maybeAutoUpdateItemDb() {
  if (!isTauri() || catalogStore.updating) return;
  try {
    if (sessionStorage.getItem(AUTO_UPDATE_KEY)) return;
    sessionStorage.setItem(AUTO_UPDATE_KEY, "1");
  } catch {
    return;
  }
  const status = catalogStore.status ?? (await itemdbStatus());
  if (!status) return;
  catalogStore.status = status;
  const age = status.age_secs;
  if (status.available && age != null && age <= 24 * 3600) return;
  await updateItemDb(false);
}
