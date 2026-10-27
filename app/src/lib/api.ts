import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface MarketItemDto {
  id: string;
  slug: string;
  name: string;
  icon: string | null;
  thumb: string | null;
  ducats: number | null;
  vaulted: boolean;
  tradable: boolean;
  max_rank: number | null;
  max_charges: number | null;
  max_amber_stars: number | null;
  max_cyan_stars: number | null;
  subtypes: string[];
  tags: string[];
  bulk_tradable: boolean;
  trading_tax: number | null;
}

export interface VariantDto {
  rank: number | null;
  charges: number | null;
  amber_stars: number | null;
  cyan_stars: number | null;
  subtype: string | null;
  platinum: number;
  online: number;
  quantity: number;
}

export interface QuoteDto {
  slug: string;
  platinum: number | null;
  online: number;
  approx: boolean;
  updated_at: number;
  history: number[];
  variants: VariantDto[];
}

export interface TradeItemDto {
  name: string;
  quantity: number;
  rank: number | null;
  platinum: boolean;
}

export interface TradeEventDto {
  player: string;
  offering: TradeItemDto[];
  receiving: TradeItemDto[];
}

export interface OrderDto {
  platinum: number;
  unit_price: number;
  quantity: number;
  per_trade: number;
  ingame_name: string;
  status: string;
  rank: number | null;
  charges: number | null;
  amber_stars: number | null;
  cyan_stars: number | null;
  subtype: string | null;
}

export interface OrderFilter {
  rank?: number | null;
  charges?: number | null;
  amber_stars?: number | null;
  cyan_stars?: number | null;
  subtype?: string | null;
}

export interface MarketFocusDto {
  item: MarketItemDto;
  quote: QuoteDto;
  sell: OrderDto[];
  buy: OrderDto[];
  best_sell: OrderDto | null;
}

export interface MarketStatusDto {
  cached: number;
  queued: number;
  last_recent: number;
  last_error: string | null;
}

export interface TrendingDto {
  item: MarketItemDto;
  orders: number;
  quantity: number;
  min_sell: number | null;
}

export interface CycleDto {
  id: string;
  name: string;
  state: string;
  activation: string;
  expiry: string;
  time_left: string | null;
}

export interface FissureDto {
  tier: string;
  mission_type: string;
  node: string;
  expiry: string;
  is_hard: boolean;
  is_storm: boolean;
}

export interface CircuitDto {
  normal: string[];
  hard: string[];
}

export interface AlertDto {
  mission_type: string;
  node: string;
  reward: string;
  expiry: string;
}

export interface WorldstateDto {
  cycles: CycleDto[];
  fissures: FissureDto[];
  alerts: AlertDto[];
  circuit: CircuitDto;
  stale: boolean;
  updated_at: number;
  upstream: string;
}

export interface PortalDto {
  present: boolean;
  version: number | null;
  source_types: number | null;
  monitor_source: boolean;
  window_source: boolean;
  backend: string | null;
  error: string | null;
}

export interface SnapshotCapabilityDto {
  present: boolean;
  ptrace_scope: number | null;
  blocked: boolean;
}

export interface CapabilitiesDto {
  wayland: boolean;
  session_type: string;
  desktop: string;
  package: string;
  portal: PortalDto;
  pipewire: boolean;
  layer_shell: boolean | null;
  secret_service: boolean;
  tesseract: string | null;
  snapshot: SnapshotCapabilityDto;
  capture_token: boolean;
}

export type HudCorner = "top-left" | "top-right" | "bottom-left" | "bottom-right";

export interface MonitorDto {
  connector: string;
  model: string;
  primary: boolean;
  width: number;
  height: number;
  scale: number;
}

export interface HudSettingsDto {
  corner: HudCorner;
  monitor: string | null;
  monitors: MonitorDto[];
}

export interface CaptureStreamDto {
  source: string;
  width: number | null;
  height: number | null;
  node_id: number;
}

export interface CaptureSourceDto {
  streams: CaptureStreamDto[];
  token_stored: boolean;
}

export function isTauri() {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function hudSettings(): Promise<HudSettingsDto | null> {
  if (!isTauri()) return null;
  try {
    return await invoke<HudSettingsDto>("hud_settings");
  } catch {
    return null;
  }
}

export async function setHudSettings(corner: HudCorner, monitor: string | null): Promise<boolean> {
  if (!isTauri()) return false;
  try {
    await invoke("set_hud_settings", { corner, monitor });
    return true;
  } catch {
    return false;
  }
}

export interface HudProbeStatusDto {
  enabled: boolean;
  running: boolean;
  pid: number | null;
  log_path: string;
  last_error: string | null;
}

export async function hudProbeStatus(): Promise<HudProbeStatusDto | null> {
  if (!isTauri()) return null;
  try {
    return await invoke<HudProbeStatusDto>("hud_probe_status");
  } catch {
    return null;
  }
}

export async function startHudProbe(): Promise<HudProbeStatusDto> {
  if (!isTauri()) throw new Error("Not running in the app.");
  return invoke<HudProbeStatusDto>("hud_probe_start");
}

export async function stopHudProbe(): Promise<HudProbeStatusDto> {
  if (!isTauri()) throw new Error("Not running in the app.");
  return invoke<HudProbeStatusDto>("hud_probe_stop");
}

export async function hudProbeLog(): Promise<string> {
  if (!isTauri()) return "";
  try {
    return await invoke<string>("hud_probe_log");
  } catch {
    return "";
  }
}

export async function openHudLog(): Promise<boolean> {
  if (!isTauri()) return false;
  try {
    await invoke("open_hud_log");
    return true;
  } catch {
    return false;
  }
}

export async function chooseCaptureSource(): Promise<CaptureSourceDto> {
  if (!isTauri()) throw new Error("Not running in the app.");
  return invoke<CaptureSourceDto>("choose_capture_source");
}

export async function capabilities(): Promise<CapabilitiesDto | null> {
  if (!isTauri()) return null;
  try {
    return await invoke<CapabilitiesDto>("capabilities");
  } catch {
    return null;
  }
}

export async function launchCommand(): Promise<string> {
  if (!isTauri()) return "";
  try {
    return await invoke<string>("launch_command");
  } catch {
    return "";
  }
}

export async function openExternal(url: string) {
  if (isTauri()) {
    try {
      await invoke("open_url", { url });
      return;
    } catch {
      /* fall through to the browser path */
    }
  }
  window.open(url, "_blank", "noopener,noreferrer");
}

export { formatAge } from "./format";

export async function marketSearch(query: string, limit = 30): Promise<MarketItemDto[]> {
  if (!isTauri()) return [];
  return invoke<MarketItemDto[]>("market_search", { query, limit });
}

export async function marketItems(slugs: string[]): Promise<MarketItemDto[]> {
  if (!isTauri()) return [];
  return invoke<MarketItemDto[]>("market_items", { slugs });
}

export async function marketQuotes(slugs: string[]): Promise<QuoteDto[]> {
  if (!isTauri()) return [];
  return invoke<QuoteDto[]>("market_quotes", { slugs });
}

// Cached prices only. `marketQuotes` also queues a full book per slug, which a
// whole inventory must not do.
export async function marketCachedQuotes(slugs: string[]): Promise<QuoteDto[]> {
  if (!isTauri() || !slugs.length) return [];
  return invoke<QuoteDto[]>("market_cached_quotes", { slugs });
}

export interface RankedSlug {
  slug: string;
  rank: number | null;
  subtype: string | null;
}

export async function marketInventoryQuotes(
  pairs: RankedSlug[],
  refresh = false,
): Promise<QuoteDto[]> {
  if (!isTauri() || !pairs.length) return [];
  return invoke<QuoteDto[]>("market_inventory_quotes", { pairs, refresh });
}

export async function marketFocus(
  slug: string,
  filter?: OrderFilter | null,
  force = false,
): Promise<MarketFocusDto> {
  return invoke<MarketFocusDto>("market_focus", { slug, filter: filter ?? null, force });
}

export async function marketStatus(): Promise<MarketStatusDto | null> {
  if (!isTauri()) return null;
  try {
    return await invoke<MarketStatusDto>("market_status");
  } catch {
    return null;
  }
}

export async function marketTrending(): Promise<TrendingDto[]> {
  if (!isTauri()) return [];
  try {
    return await invoke<TrendingDto[]>("market_trending");
  } catch {
    return [];
  }
}

export async function onMarketTrending(
  handler: (trending: TrendingDto[]) => void,
): Promise<UnlistenFn | (() => void)> {
  if (!isTauri()) return () => {};
  return listen<TrendingDto[]>("market-trending", (event) => handler(event.payload));
}

export async function onMarketQuotes(
  handler: (quotes: QuoteDto[]) => void,
): Promise<UnlistenFn | (() => void)> {
  if (!isTauri()) return () => {};
  return listen<QuoteDto[]>("market-quotes", (event) => handler(event.payload));
}

export async function worldstate(force = false): Promise<WorldstateDto | null> {
  if (!isTauri()) return null;
  try {
    return await invoke<WorldstateDto>("worldstate", { force });
  } catch {
    return null;
  }
}

export interface ItemDto {
  unique_name: string;
  name: string;
  category: string;
  kind: string;
  item_type: string | null;
  image_url: string | null;
  tradable: boolean;
  masterable: boolean;
  mastery_req: number | null;
  vaulted: boolean;
  prime: boolean;
  build_price: number | null;
  build_time: number | null;
  build_quantity: number | null;
  skip_build_time_price: number | null;
  bp_cost: number | null;
  mastery_cap: number | null;
}

export interface ComponentDto {
  unique_name: string;
  name: string;
  count: number;
  tradable: boolean;
  market_slug: string | null;
  // Relative market asset path for the part's own art.
  image?: string | null;
  icon?: string;
}

export interface CatalogCardDto {
  item: ItemDto;
  market: MarketItemDto | null;
  components: ComponentDto[];
}

export interface ItemDbStatusDto {
  available: boolean;
  path: string;
  items: number;
  age_secs: number | null;
  fetched_at: number;
  source: string;
  categories: Record<string, number>;
  mastery_available: boolean;
  error: string | null;
}

export interface SetRefDto {
  name: string;
  slug: string;
  have: number;
  need: number;
}

export interface InventoryItemDto {
  item: ItemDto;
  market: MarketItemDto | null;
  count: number;
  kind: string;
  sellable: boolean;
  built: boolean;
  rank: number | null;
  tags: string[];
  mastered: boolean | null;
  resolved: boolean;
  subtype: string | null;
  set: SetRefDto | null;
}

export interface MasteryDto {
  xp: Record<string, number>;
  mastered: string[];
}

export interface ArrayStatusDto {
  source: string;
  observed_at: number | null;
}

// Mirrors crates/core/src/snapshot_status.rs. The writer bumps both values; the
// app only warns when a snapshot is newer than it understands.
export const SNAPSHOT_SCHEMA = 3;

export interface LastScanDto {
  complete: boolean;
  limit: string | null;
  limit_blocking: boolean;
  trigger: string;
  fast_path: string;
  bytes: number;
  extraction_bytes: number;
  regions: number;
  duration_ms: number;
  error: string | null;
}

export interface InventoryStatusDto {
  version: number;
  snapshot_schema: number;
  content_hash: string;
  captured_at: number;
  verified_at: number;
  last_scan_at: number;
  previous_unreadable: boolean;
  game_pid: number;
  fast_cluster: [number, number] | null;
  fast_regions: [number, number][];
  last_scan: LastScanDto;
}

export interface InventorySnapshotDto {
  linked: boolean;
  captured_at: number;
  complete: boolean;
  captured: string[];
  missing: string[];
  schema: number | null;
  limit: string | null;
  array_status: Record<string, ArrayStatusDto>;
  total: number;
  sellable: number;
  unresolved: number;
  mastery_available: boolean;
  mastery: MasteryDto;
  items: InventoryItemDto[];
  component_counts: Record<string, number>;
}

export interface RelicRewardDto {
  name: string;
  slug: string | null;
  chance: number;
  ducats: number | null;
}

export interface RelicEntryDto {
  tier: string;
  name: string;
  state: string;
  rewards: RelicRewardDto[];
}

export async function relicEntries(names: string[]): Promise<RelicEntryDto[]> {
  if (!isTauri() || !names.length) return [];
  return invoke<RelicEntryDto[]>("relic_entries", { names });
}

export async function catalogCards(): Promise<CatalogCardDto[]> {
  if (!isTauri()) return [];
  return invoke<CatalogCardDto[]>("catalog_cards");
}

export async function itemdbStatus(): Promise<ItemDbStatusDto | null> {
  if (!isTauri()) return null;
  try {
    return await invoke<ItemDbStatusDto>("itemdb_status");
  } catch {
    return null;
  }
}

export async function itemdbUpdate(force = false): Promise<ItemDbStatusDto | null> {
  if (!isTauri()) return null;
  return invoke<ItemDbStatusDto>("itemdb_update", { force });
}

// Existence only: the snapshot's contents are never read for the consent check.
export async function inventorySnapshotAvailable(): Promise<boolean> {
  if (!isTauri()) return false;
  try {
    return await invoke<boolean>("inventory_snapshot_available");
  } catch {
    return false;
  }
}

// Seconds since the epoch; 0 when the file is missing.
export async function inventorySnapshotStamp(): Promise<number> {
  if (!isTauri()) return 0;
  try {
    return await invoke<number>("inventory_snapshot_stamp");
  } catch {
    return 0;
  }
}

export async function inventoryItems(): Promise<InventorySnapshotDto | null> {
  if (!isTauri()) return null;
  try {
    return await invoke<InventorySnapshotDto>("inventory_items");
  } catch {
    return null;
  }
}

// None when the wrapper predates the status file; callers fall back to the
// snapshot mtime.
export async function inventoryStatus(): Promise<InventoryStatusDto | null> {
  if (!isTauri()) return null;
  try {
    return await invoke<InventoryStatusDto>("inventory_status");
  } catch {
    return null;
  }
}

export async function requestInventoryScan(rewrite = false): Promise<void> {
  await invoke("request_inventory_scan", { rewrite });
}

export interface AccountStatusDto {
  connected: boolean;
  verified: boolean;
  last_error: string | null;
  // "wallet" | "file" | "none" | "unknown" (unknown = not loaded yet).
  storage: string;
  needs_unlock: boolean;
  warning: string | null;
}

export interface MyOrderDto {
  id: string;
  item_id: string;
  item: MarketItemDto | null;
  kind: string;
  platinum: number;
  quantity: number;
  visible: boolean;
  per_trade: number | null;
  rank: number | null;
  charges: number | null;
  amber_stars: number | null;
  cyan_stars: number | null;
  subtype: string | null;
  created_at: string;
  updated_at: string;
}

export interface OrderInputDto {
  item_id: string;
  platinum: number;
  quantity: number;
  visible?: boolean;
  rank?: number | null;
  charges?: number | null;
  amber_stars?: number | null;
  cyan_stars?: number | null;
  subtype?: string | null;
  per_trade?: number | null;
}

const DISCONNECTED: AccountStatusDto = {
  connected: false,
  verified: false,
  last_error: null,
  storage: "unknown",
  needs_unlock: true,
  warning: null,
};

export async function marketAccountStatus(force = false): Promise<AccountStatusDto> {
  if (!isTauri()) return DISCONNECTED;
  try {
    return await invoke<AccountStatusDto>("market_account_status", { force });
  } catch (error) {
    return { ...DISCONNECTED, last_error: String(error) };
  }
}

// The one-shot background wallet load. Single-flight in the backend; a manual
// force bypasses the account-configured marker.
export async function marketAccountLoad(force = false): Promise<AccountStatusDto> {
  if (!isTauri()) return DISCONNECTED;
  return invoke<AccountStatusDto>("market_account_load", { force });
}

export async function onAccountStatus(
  handler: (status: AccountStatusDto) => void,
): Promise<UnlistenFn | (() => void)> {
  if (!isTauri()) return () => {};
  return listen<AccountStatusDto>("account-status", (event) => handler(event.payload));
}

export async function marketAccountLogin(
  email: string,
  password: string,
): Promise<AccountStatusDto> {
  return invoke<AccountStatusDto>("market_account_login", { email, password });
}

export async function marketAccountConnect(token: string): Promise<AccountStatusDto> {
  return invoke<AccountStatusDto>("market_account_connect", { token });
}

export async function marketAccountDisconnect(): Promise<AccountStatusDto> {
  if (!isTauri()) return DISCONNECTED;
  return invoke<AccountStatusDto>("market_account_disconnect");
}

export interface PresenceDto {
  connected: boolean;
  error: string | null;
  effective: string | null;
}// Presence lives in the backend: warframe.market rejects the webview origin.
export async function marketSetPresence(
  status: string | null,
  reconnect = false,
): Promise<void> {
  if (!isTauri()) return;
  await invoke("market_set_presence", { status, reconnect });
}

export async function marketPresenceStatus(): Promise<PresenceDto | null> {
  if (!isTauri()) return null;
  try {
    return await invoke<PresenceDto>("market_presence_status");
  } catch {
    return null;
  }
}

export async function onPresence(
  handler: (state: PresenceDto) => void,
): Promise<UnlistenFn | (() => void)> {
  if (!isTauri()) return () => {};
  return listen<PresenceDto>("wfm-presence", (event) => handler(event.payload));
}

export async function marketMyOrders(): Promise<MyOrderDto[]> {
  if (!isTauri()) return [];
  return invoke<MyOrderDto[]>("market_my_orders");
}

export async function marketOrderCreate(input: OrderInputDto): Promise<string | null> {
  return invoke<string | null>("market_order_create", { input });
}

export async function marketOrderUpdate(id: string, input: OrderInputDto): Promise<void> {
  await invoke("market_order_update", { id, input });
}

export async function marketOrderClose(id: string, quantity: number): Promise<void> {
  await invoke("market_order_close", { id, quantity });
}

export async function marketOrderDelete(id: string): Promise<void> {
  await invoke("market_order_delete", { id });
}

export interface RivenStatDto {
  name: string;
  slug: string;
  value: number;
  unit: string;
  positive: boolean;
}

export interface RivenDto {
  id: string;
  name: string;
  weapon: string | null;
  weapon_slug: string | null;
  weapon_type: string | null;
  weapon_thumb: string | null;
  polarity: string;
  rank: number;
  mastery: number | null;
  rerolls: number;
  veiled: boolean;
  challenge: string | null;
  stats: RivenStatDto[];
}

export interface RivenListingStatDto {
  name: string;
  url_name: string;
  value: number;
  unit: string;
  positive: boolean;
}

export interface RivenListingDto {
  id: string;
  direct: boolean;
  starting_price: number;
  buyout_price: number | null;
  top_bid: number | null;
  created: string;
  visible: boolean;
  note: string;
  name: string;
  weapon: string | null;
  weapon_slug: string;
  weapon_thumb: string | null;
  rank: number | null;
  rerolls: number;
  mastery: number | null;
  polarity: string | null;
  stats: RivenListingStatDto[];
}

export async function rivenInventory(): Promise<RivenDto[]> {
  if (!isTauri()) return [];
  return invoke<RivenDto[]>("riven_inventory");
}

export async function rivenListings(): Promise<RivenListingDto[]> {
  if (!isTauri()) return [];
  return invoke<RivenListingDto[]>("riven_listings");
}
