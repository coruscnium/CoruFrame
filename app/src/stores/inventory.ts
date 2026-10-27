import { shallowReactive } from "vue";
import {
  inventoryItems,
  inventorySnapshotStamp,
  inventoryStatus,
  requestInventoryScan,
  isTauri,
  SNAPSHOT_SCHEMA,
  type InventoryItemDto,
  type InventorySnapshotDto,
  type InventoryStatusDto,
  type MarketItemDto,
  type RankedSlug,
} from "../lib/api";
import { inventory as mockInventory, market as mockMarket } from "../data/mock";
import { app } from "./app";
import { clearQuoteCache, ensureRankedQuotes, loadCachedQuotes } from "./market";
import { snapshotChecking, snapshotEnabled } from "./ui";

export interface SellTarget {
  market: MarketItemDto;
  name: string;
  count: number;
  rank: number | null;
  subtype: string | null;
}

export const inventory = shallowReactive({
  snapshot: null as InventorySnapshotDto | null,
  // The wrapper's last-scan contract. It may be newer than the loaded snapshot
  // (a partial scan does not change the raw), so it is display state only.
  status: null as InventoryStatusDto | null,
  // Content hash of the loaded raw; the watcher reloads only when it changes.
  knownHash: "",
  loading: false,
  rescanning: false,
  error: "",
  // The view waits for this before sorting by price: an unpriced row would
  // otherwise sort last, never render, and never be requested.
  pricesReady: false,
  pricesError: "",
  initialized: false,
});

export function snapshotSchema(): number | null {
  return inventory.status?.snapshot_schema ?? inventory.snapshot?.schema ?? null;
}

export function snapshotSchemaIsNewer(): boolean {
  const schema = snapshotSchema();
  return schema != null && schema > SNAPSHOT_SCHEMA;
}

function snapshotErrorText(): string {
  if (inventory.status?.previous_unreadable) return "Snapshot unreadable";
  return "Inventory snapshot unavailable";
}

function slugify(name: string) {
  return name.toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_|_$/g, "");
}

function mockMarketItem(name: string): MarketItemDto {
  const row = mockMarket.find((item) => item.name === name);
  return {
    id: row?.id ?? slugify(name),
    slug: row?.slug ?? slugify(name),
    name,
    icon: null,
    thumb: null,
    ducats: null,
    vaulted: false,
    tradable: true,
    max_rank: null,
    max_charges: null,
    max_amber_stars: null,
    max_cyan_stars: null,
    subtypes: [],
    tags: [],
    bulk_tradable: false,
    trading_tax: null,
  };
}

function mockSnapshot(): InventorySnapshotDto {
  const items: InventoryItemDto[] = mockInventory.map((row) => {
    const market = row.price > 0 ? mockMarketItem(row.name) : null;
    const kind = row.category === "Resource" ? "resource" : "component";
    return {
      item: {
        unique_name: `mock:${slugify(row.name)}`,
        name: row.name,
        category: "Mock",
        kind,
        item_type: null,
        image_url: null,
        tradable: market != null,
        masterable: false,
        mastery_req: null,
        vaulted: row.vaulted,
        prime: row.name.includes("Prime"),
        build_price: null,
        build_time: null,
        build_quantity: null,
        skip_build_time_price: null,
        bp_cost: null,
        mastery_cap: null,
      },
      market,
      count: row.count,
      kind,
      sellable: row.price > 0,
      built: false,
      rank: null,
      tags: [],
      mastered: null,
      resolved: true,
      subtype: null,
      set: null,
    };
  });
  return {
    linked: true,
    captured_at: Math.floor(Date.now() / 1000),
    complete: true,
    captured: [],
    missing: [],
    schema: SNAPSHOT_SCHEMA,
    limit: null,
    array_status: {},
    total: items.length,
    sellable: items.filter((item) => item.sellable).length,
    unresolved: 0,
    mastery_available: false,
    mastery: { xp: {}, mastered: [] },
    items,
    component_counts: {},
  };
}

function snapshotPairs(snapshot: InventorySnapshotDto): RankedSlug[] {
  const seen = new Set<string>();
  const pairs: RankedSlug[] = [];
  const add = (slug: string, rank: number | null, subtype: string | null) => {
    const key = `${slug}\u0000${rank ?? "all"}\u0000${subtype ?? "any"}`;
    if (seen.has(key)) return;
    seen.add(key);
    pairs.push({ slug, rank, subtype });
  };
  for (const item of snapshot.items) {
    if (item.sellable && item.market) add(item.market.slug, item.rank, item.subtype);
    if (item.set) add(item.set.slug, null, null);
  }
  return pairs;
}

function snapshotSlugs(snapshot: InventorySnapshotDto): string[] {
  return [...new Set(snapshotPairs(snapshot).map((pair) => pair.slug))];
}

// One bulk read of the persisted price cache. The background queue handles
// everything that is genuinely missing; this only makes the first paint whole.
async function loadPrices(snapshot: InventorySnapshotDto) {
  inventory.pricesError = "";
  try {
    await loadCachedQuotes(snapshotSlugs(snapshot));
  } catch (error) {
    inventory.pricesError = String(error);
    // The cached read is what makes pricing viewport-independent. If it is
    // unavailable, ask for the whole collection at once instead of letting
    // rows wait for a scroll to be requested.
    void ensureRankedQuotes(snapshotPairs(snapshot));
  } finally {
    inventory.pricesReady = true;
  }
}

// A refresh that arrives during a load must not vanish: queue one forced run
// and start it when the current request finishes.
let pendingForce = false;

let snapshotWatch: number | null = null;
let snapshotStamp = 0;

// The wrapper writes the snapshot in another process. The small status file is
// read every tick; a reload happens only when the content hash changes, so a
// verified-only scan does not re-read the raw or re-warm prices.
export function startInventoryWatch() {
  if (!isTauri() || snapshotWatch !== null) return;
  const tick = async () => {
    if (!snapshotEnabled.value) return;
    const status = await inventoryStatus();
    if (status) {
      inventory.status = status;
      if (!status.content_hash) return;
      if (!inventory.knownHash) {
        // A failed read (for example a corrupt raw) must keep retrying; a
        // healthy first read adopts the current hash without a second reload.
        if (inventory.error) {
          void loadInventory(true);
          return;
        }
        inventory.knownHash = status.content_hash;
        return;
      }
      if (inventory.knownHash !== status.content_hash) {
        inventory.knownHash = status.content_hash;
        void loadInventory(true);
      }
      return;
    }
    // An older wrapper has no status file: keep the mtime fallback.
    const stamp = await inventorySnapshotStamp();
    if (!stamp) return;
    if (snapshotStamp === 0) {
      snapshotStamp = stamp;
      return;
    }
    if (stamp > snapshotStamp) {
      snapshotStamp = stamp;
      void loadInventory(true);
    }
  };
  snapshotWatch = window.setInterval(() => void tick(), 5000);
  void tick();
}

export async function rescanInventory(rewrite = false) {
  if (!isTauri()) {
    await loadInventory(true);
    return;
  }
  if (inventory.rescanning) return;
  inventory.rescanning = true;
  inventory.error = "";
  const before = inventory.status?.last_scan_at ?? 0;
  try {
    await requestInventoryScan(rewrite);
    for (let attempt = 0; attempt < 24; attempt += 1) {
      await new Promise((resolve) => setTimeout(resolve, 500));
      const status = await inventoryStatus();
      if (!status) continue;
      inventory.status = status;
      if ((status.last_scan_at ?? 0) > before) {
        await loadInventory(true);
        return;
      }
    }
    inventory.error = "The wrapper did not answer. Is Warframe running through it?";
  } catch (error) {
    inventory.error = String(error);
  } finally {
    inventory.rescanning = false;
  }
}

export async function loadInventory(force = false) {
  // The consent check owns whether the backend is read at all; every caller
  // (App, Inventory, Trading, Collection, Relic planner) goes through here.
  if (snapshotChecking.value) return;
  if (!snapshotEnabled.value) {
    inventory.snapshot = null;
    inventory.status = null;
    inventory.knownHash = "";
    inventory.error = "";
    inventory.pricesReady = true;
    inventory.initialized = false;
    app.snapshotLinked = false;
    app.snapshotChecked = true;
    clearQuoteCache();
    return;
  }
  if (inventory.loading) {
    if (force) pendingForce = true;
    return;
  }
  if (inventory.initialized && !force) return;
  inventory.loading = true;
  inventory.error = "";
  if (!isTauri()) {
    inventory.snapshot = mockSnapshot();
    app.snapshotLinked = true;
    app.snapshotChecked = true;
    inventory.pricesReady = true;
    inventory.initialized = true;
    inventory.loading = false;
    if (pendingForce) {
      pendingForce = false;
      void loadInventory(true);
    }
    return;
  }
  try {
    // Read the status before the raw: if a scan lands between the two reads, a
    // later tick sees the new hash and reloads rather than missing it.
    const status = await inventoryStatus();
    if (status) inventory.status = status;
    const snapshot = await inventoryItems();
    if (snapshot) {
      const previous = inventory.snapshot;
      if (previous?.complete && !snapshot.complete) {
        // Keep the last complete counts; only the scan status got worse.
        inventory.snapshot = {
          ...previous,
          complete: false,
          captured: snapshot.captured,
          missing: snapshot.missing,
        };
      } else {
        inventory.snapshot = snapshot;
      }
      app.snapshotLinked = snapshot.linked;
      // A manual reload adopts the current content hash so the next tick does
      // not reload the same content again.
      if (status) inventory.knownHash = status.content_hash;
      const current = inventory.snapshot;
      if (current) await loadPrices(current);
    } else {
      inventory.error = snapshotErrorText();
      inventory.knownHash = "";
      inventory.pricesReady = true;
    }
  } catch (error) {
    inventory.error = String(error);
    inventory.knownHash = "";
    inventory.pricesReady = true;
  } finally {
    inventory.loading = false;
    inventory.initialized = true;
    app.snapshotChecked = true;
    if (pendingForce) {
      pendingForce = false;
      void loadInventory(true);
    }
  }
}
