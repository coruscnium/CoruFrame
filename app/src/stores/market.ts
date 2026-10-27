import { reactive, shallowReactive } from "vue";
import {
  isTauri,
  marketCachedQuotes,
  marketFocus,
  marketInventoryQuotes,
  marketItems,
  marketQuotes,
  marketSearch,
  marketStatus,
  marketTrending,
  onMarketQuotes,
  onMarketTrending,
  type MarketFocusDto,
  type MarketItemDto,
  type MarketStatusDto,
  type OrderFilter,
  type RankedSlug,
  type QuoteDto,
  type TrendingDto,
} from "../lib/api";
import { market as mockRows } from "../data/mock";
import { app } from "./app";

const SEED = [
  "braton_prime_barrel",
  "lex_prime_blueprint",
  "paris_prime_upper_limb",
  "wukong_prime_neuroptics_blueprint",
  "sevagoth_prime_neuroptics_blueprint",
  "kompressa_prime_blueprint",
  "dual_zoren_prime_blueprint",
];
const STORAGE_KEY = "coruframe.market.tracked";

export const market = reactive({
  query: "",
  results: [] as MarketItemDto[],
  tracked: [] as string[],
  items: {} as Record<string, MarketItemDto>,
  quotes: shallowReactive<Record<string, QuoteDto>>({}),
  trending: [] as TrendingDto[],
  active: null as MarketItemDto | null,
  activeFilter: null as OrderFilter | null,
  focus: null as MarketFocusDto | null,
  loading: false,
  focusLoading: false,
  error: "",
  status: null as MarketStatusDto | null,
  initialized: false,
  usingMock: false,
  lastUpdated: 0,
  version: 0,
});

let initPromise: Promise<void> | null = null;
let focusSeq = 0;
let searchSeq = 0;

function nowSeconds() {
  return Math.floor(Date.now() / 1000);
}

// Market variants are priced separately: max-rank arcanes/mods and fully
// starred sculptures are the common default, but every tier stays selectable.
export function defaultFilter(item: MarketItemDto): OrderFilter {
  return {
    rank: item.max_rank ?? null,
    charges: item.max_charges ?? null,
    amber_stars: item.max_amber_stars ?? null,
    cyan_stars: item.max_cyan_stars ?? null,
    subtype: item.subtypes[0] ?? null,
  };
}

function loadTracked(): string[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    // An empty saved list means the user untracked everything; only a missing
    // key is a fresh install that gets the seed.
    if (raw !== null) {
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed)) {
        return parsed.filter((value): value is string => typeof value === "string");
      }
    }
  } catch {
    /* localStorage can be unavailable; the seed still works */
  }
  return [...SEED];
}

function saveTracked() {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(market.tracked));
  } catch {
    /* non-fatal */
  }
}

function mergeItems(items: MarketItemDto[]) {
  for (const item of items) {
    market.items[item.slug] = item;
  }
}

function mergeQuotes(quotes: QuoteDto[]) {
  if (!quotes.length) return;
  let changed = false;
  for (const quote of quotes) {
    const previous = market.quotes[quote.slug];
    // A bulk cached read can land after a fresher background event; the newer
    // record wins so prices and history cannot go backwards.
    if (previous && previous.updated_at > quote.updated_at) continue;
    market.quotes[quote.slug] = quote;
    if (quote.updated_at > market.lastUpdated) market.lastUpdated = quote.updated_at;
    changed = true;
  }
  if (changed) market.version += 1;
}

const CACHED_CHUNK = 500;

// One quiet IPC per chunk fills the store from disk before any network work.
// Chunking keeps the largest case (the whole catalog) from building a single
// multi-megabyte payload.
export async function loadCachedQuotes(slugs: string[]) {
  if (!isTauri()) return;
  const unique = [...new Set(slugs.filter(Boolean))];
  for (let index = 0; index < unique.length; index += CACHED_CHUNK) {
    mergeQuotes(await marketCachedQuotes(unique.slice(index, index + CACHED_CHUNK)));
  }
}

function initMock() {
  const now = nowSeconds();
  const items: MarketItemDto[] = mockRows.map((row) => ({
    id: row.id,
    slug: row.slug,
    name: row.name,
    icon: null,
    thumb: null,
    ducats: null,
    vaulted: false,
    tradable: row.name !== "Forma Blueprint",
    max_rank: null,
    max_charges: null,
    max_amber_stars: null,
    max_cyan_stars: null,
    subtypes: [],
    tags: [],
    bulk_tradable: false,
    trading_tax: null,
  }));
  items.forEach((item, index) => {
    market.items[item.slug] = item;
    const row = mockRows[index];
    market.quotes[item.slug] = {
      slug: item.slug,
      platinum: row.price > 0 ? row.price : null,
      online: row.online,
      approx: false,
      updated_at: now,
      history: [...row.points],
      variants: [],
    };
  });
  market.tracked = items.map((item) => item.slug);
  market.trending = items.slice(0, 6).map((item, index) => ({
    item,
    orders: mockRows[index]?.online ?? 0,
    quantity: mockRows[index]?.online ?? 0,
    min_sell: mockRows[index]?.price || null,
  }));
  market.status = { cached: items.length, queued: 0, last_recent: now, last_error: null };
  market.usingMock = true;
  market.initialized = true;
}

export function initMarket() {
  if (initPromise) return initPromise;
  initPromise = (async () => {
    if (!isTauri()) {
      initMock();
      return;
    }
    market.tracked = loadTracked();
    await onMarketQuotes((quotes) => mergeQuotes(quotes));
    await onMarketTrending((trending) => {
      market.trending = trending;
      mergeItems(trending.map((row) => row.item));
    });
    try {
      const [items, quotes, status, trending] = await Promise.all([
        marketItems(market.tracked),
        marketQuotes(market.tracked),
        marketStatus(),
        marketTrending(),
      ]);
      mergeItems(items);
      mergeQuotes(quotes);
      if (status) market.status = status;
      if (trending.length) {
        market.trending = trending;
        mergeItems(trending.map((row) => row.item));
      }
    } catch (error) {
      market.error = String(error);
    }
    market.initialized = true;
    window.setInterval(async () => {
      const status = await marketStatus();
      if (status) market.status = status;
    }, 15000);
  })();
  return initPromise;
}

export async function searchMarket(query: string) {
  const seq = ++searchSeq;
  market.query = query;
  // Typing in the search box is an explicit request for results, not for the
  // previously selected item's order book.
  clearActive();
  const trimmed = query.trim();
  if (!trimmed) {
    market.results = [];
    return;
  }
  market.loading = true;
  market.error = "";
  try {
    if (!isTauri()) {
      const lower = trimmed.toLowerCase();
      market.results = Object.values(market.items)
        .filter((item) => item.name.toLowerCase().includes(lower))
        .slice(0, 24);
      return;
    }
    const results = await marketSearch(trimmed, 24);
    // A slower earlier query must not overwrite the newer results.
    if (seq !== searchSeq) return;
    market.results = results;
    mergeItems(results);
    const quotes = await marketQuotes(results.map((item) => item.slug));
    if (seq !== searchSeq) return;
    mergeQuotes(quotes);
  } catch (error) {
    if (seq === searchSeq) market.error = String(error);
  } finally {
    if (seq === searchSeq) market.loading = false;
  }
}

export async function focusItem(item: MarketItemDto, filter?: OrderFilter, force = false) {
  const seq = ++focusSeq;
  const applied = filter ?? defaultFilter(item);
  market.active = item;
  market.activeFilter = applied;
  market.focus = null;
  market.error = "";
  market.focusLoading = true;
  // Seed the headline, history and variants from disk so only the order book
  // is missing while the network call runs.
  if (isTauri() && !market.quotes[item.slug]) {
    try {
      await loadCachedQuotes([item.slug]);
    } catch {
      /* the book fetch below still runs */
    }
    if (seq !== focusSeq) return;
  }
  try {
    if (!isTauri()) {
      const row = mockRows.find((candidate) => candidate.slug === item.slug);
      const quote = market.quotes[item.slug];
      if (seq === focusSeq && quote) {
        market.focus = {
          item,
          quote,
          sell: row
            ? [
                {
                  platinum: row.price,
                  unit_price: row.price,
                  quantity: 1,
                  per_trade: 1,
                  ingame_name: "preview",
                  status: "online",
                  rank: null,
                  charges: null,
                  amber_stars: null,
                  cyan_stars: null,
                  subtype: null,
                },
              ]
            : [],
          buy: [],
          best_sell: null,
        };
      }
      return;
    }
    const focus = await marketFocus(item.slug, applied, force);
    if (seq !== focusSeq) return;
    market.focus = focus;
    mergeItems([focus.item]);
    mergeQuotes([focus.quote]);
  } catch (error) {
    if (seq === focusSeq) market.error = String(error);
  } finally {
    if (seq === focusSeq) market.focusLoading = false;
  }
}

export async function focusByCatalogName(name: string) {
  const query = name.replace(/\s+Set$/i, "").trim();
  if (!query) return;
  market.query = "";
  market.results = [];
  if (!isTauri()) {
    const lower = query.toLowerCase();
    const item = Object.values(market.items).find(
      (candidate) =>
        candidate.name.toLowerCase() === lower ||
        candidate.name.toLowerCase() === `${lower} set` ||
        candidate.name.toLowerCase().startsWith(lower),
    );
    if (item) await focusItem(item);
    return;
  }
  try {
    const results = await marketSearch(query, 12);
    mergeItems(results);
    const lower = query.toLowerCase();
    const exact =
      results.find((item) => item.name.toLowerCase() === `${lower} set`) ??
      results.find((item) => item.name.toLowerCase() === lower) ??
      results.find((item) => item.name.toLowerCase().startsWith(lower)) ??
      results[0];
    if (exact) await focusItem(exact);
  } catch (error) {
    market.error = String(error);
  }
}

// Used by completion rows: show a part in the rail without replacing the card
// the user is reading, so their article stays selected.
export async function focusMarketSlug(slug: string, filter?: OrderFilter) {
  if (!slug) return;
  app.railTab = "market";
  if (window.innerWidth < 1220) app.railOpen = true;
  if (!isTauri()) {
    const item = Object.values(market.items).find((candidate) => candidate.slug === slug);
    if (item) await focusItem(item, filter);
    return;
  }
  try {
    const [item] = await marketItems([slug]);
    if (!item) return;
    mergeItems([item]);
    await focusItem(item, filter);
  } catch (error) {
    market.error = String(error);
  }
}

export async function ensureQuotes(slugs: string[]) {
  if (!isTauri()) return;
  const unique = [...new Set(slugs.filter(Boolean))];
  if (!unique.length) return;
  try {
    mergeQuotes(await marketQuotes(unique));
  } catch {
    /* the background queue retries */
  }
}

// Inventory rows are priced per rank; the backend answers with cached quotes
// and refreshes the rest with the cheap top-orders endpoint. `refresh` marks
// the rows on screen for immediate revalidation.
export async function ensureRankedQuotes(pairs: RankedSlug[], refresh = false) {
  if (!isTauri()) return;
  const seen = new Set<string>();
  const unique = pairs.filter((pair) => {
    const key = `${pair.slug}\u0000${pair.rank ?? "all"}\u0000${pair.subtype ?? "any"}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
  if (!unique.length) return;
  try {
    mergeQuotes(await marketInventoryQuotes(unique, refresh));
  } catch {
    /* the background queue retries */
  }
}

export function isTracked(slug: string) {
  return market.tracked.includes(slug);
}

export async function track(slug: string) {
  if (!market.tracked.includes(slug)) {
    market.tracked.push(slug);
    saveTracked();
  }
  if (isTauri()) {
    mergeItems(await marketItems([slug]));
    mergeQuotes(await marketQuotes([slug]));
  }
  const item = market.items[slug];
  if (item && !market.active) await focusItem(item);
}

export function untrack(slug: string) {
  market.tracked = market.tracked.filter((value) => value !== slug);
  saveTracked();
  if (market.active?.slug === slug) {
    market.active = null;
    market.focus = null;
  }
}

export async function refreshActive(filter?: OrderFilter) {
  if (market.active) await focusItem(market.active, filter);
}

// Turning the inventory snapshot off must not leave the previous collection's
// prices in memory.
export function clearQuoteCache() {
  for (const key of Object.keys(market.quotes)) delete market.quotes[key];
  market.version += 1;
}

export function clearActive() {
  focusSeq += 1;
  market.active = null;
  market.activeFilter = null;
  market.focus = null;
  market.focusLoading = false;
}

export function quoteDelta(quote: QuoteDto | null | undefined) {
  if (!quote || quote.history.length < 2) return 0;
  const first = quote.history[0];
  const last = quote.history[quote.history.length - 1];
  return first > 0 ? ((last - first) / first) * 100 : 0;
}

// One relative path under the market asset host. Absolute URLs, traversal and
// control characters are rejected so a remote catalog cannot choose the host.
export function iconUrl(icon: string | null) {
  if (!icon) return null;
  const value = icon.trim();
  if (!value || value.length > 300 || value.startsWith("/") || /[\u0000-\u001f\u007f\\]/.test(value)) {
    return null;
  }
  const segments = value.split("/");
  if (segments.some((segment) => !segment || segment === "." || segment === "..")) {
    return null;
  }
  return `https://warframe.market/static/assets/${segments.map(encodeURIComponent).join("/")}`;
}

export function relativeTime(seconds: number) {
  if (!seconds) return "not priced yet";
  const age = Math.max(0, nowSeconds() - seconds);
  if (age < 60) return `${age}s ago`;
  if (age < 3600) return `${Math.floor(age / 60)}m ago`;
  return `${Math.floor(age / 3600)}h ago`;
}
